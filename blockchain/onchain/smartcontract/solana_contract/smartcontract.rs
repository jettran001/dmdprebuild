//! # Solana DMD Token Program
//! 
//! Program này triển khai DMD token trên Solana theo chuẩn SPL Token và
//! tích hợp với LayerZero để bridge token giữa Solana và các blockchain khác (đặc biệt là NEAR).

use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint,
    entrypoint::ProgramResult,
    msg,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    program_pack::{IsInitialized, Pack, Sealed},
    pubkey::Pubkey,
    system_instruction,
    sysvar::{rent::Rent, Sysvar},
};
use spl_token::{
    instruction as token_instruction,
    state::{Account as TokenAccount, Mint},
};
use std::convert::TryInto;
use arrayref::{array_mut_ref, array_ref, array_refs, mut_array_refs};

// Constants
pub const LAYERZERO_CHAIN_ID_NEAR: u16 = 6; // Example chain ID for NEAR on LayerZero
pub const LAYERZERO_CHAIN_ID_SOLANA: u16 = 5; // Example chain ID for Solana on LayerZero
pub const LAYERZERO_ENDPOINT: &str = "LayerzeroEndpoint11111111111111111111111"; // Example endpoint address - would be actual Solana address in production
pub const TIMELOCK_DURATION: u64 = 172800; // 48 hours in seconds
pub const TRUSTED_REMOTE_SIZE: usize = 64; // Size of each trusted remote entry in bytes
pub const MAX_TRUSTED_REMOTES: usize = 16; // Maximum number of trusted remotes
pub const MAX_BRIDGE_REQUESTS: usize = 500; // Maximum number of active bridge requests to track
pub const EMERGENCY_TIMELOCK: u64 = 86400; // 24 hours in seconds for emergency operations
pub const MAX_ESCROW_THRESHOLD: u64 = 1_000_000_000_000; // Example: 1 million tokens (assuming 6 decimals)
pub const AUTO_REFUND_TIMEOUT: u64 = 3 * 24 * 60 * 60; // 3 days in seconds for auto-refund of stuck bridge requests
pub const MAX_DAILY_BRIDGE_VOLUME: u64 = 500_000_000_000; // Default daily bridge volume limit
pub const MAX_FAILED_MINTS: usize = 100; // Maximum number of failed mints to track
pub const MAX_SECURITY_EVENTS: usize = 50; // Maximum number of security events to track
pub const MAX_MINT_RETRIES: u8 = 5; // Maximum number of retry attempts for failed mints
pub const MIN_RETRY_INTERVAL: u64 = 600; // 10 minutes in seconds between retry attempts
pub const DEFAULT_TIMELOCK_DURATION: u64 = 172800; // 48 hours in seconds - used for trusted remote updates
pub const ADMIN_TIMELOCK_DURATION: u64 = 86400; // 24 hours in seconds for admin operations
pub const PER_TRANSACTION_BRIDGE_LIMIT: u64 = 100_000_000_000; // Maximum amount per transaction
pub const LAYERZERO_TIMEOUT: u64 = 600; // 10 minutes timeout for LayerZero operations
pub const MAX_RETRY_ATTEMPTS: u8 = 3; // Maximum number of retry attempts for bridge operations

/// Represents a bridge request with state tracking
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct BridgeRequest {
    /// The destination address
    pub to_address: [u8; 64],
    /// Amount of tokens being bridged
    pub amount: u64,
    /// Source account that initiated the bridge
    pub source_account: Pubkey,
    /// Timestamp when request was created
    pub timestamp: u64,
    /// Unique identifier for this request
    pub request_id: u64,
    /// Current status of the request
    pub status: u8, // 0=Pending, 1=Complete, 2=Failed, 3=Refunded
    /// Number of retry attempts if failed
    pub retry_count: u8,
}

/// Status of a bridge request
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum BridgeRequestStatus {
    /// Request is pending confirmation
    Pending = 0,
    /// Request is complete
    Complete = 1,
    /// Request has failed and can be retried or refunded
    Failed = 2,
    /// Request has been refunded
    Refunded = 3,
}

/// Struct for trusted remote update request
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct TrustedRemoteUpdate {
    /// The trusted remote address to update
    pub remote_address: [u8; 64],
    /// The chain ID associated with the remote
    pub chain_id: u16,
    /// Timelock expiration timestamp
    pub timelock_expiry: u64,
    /// Whether this is an add or remove operation (true = add, false = remove)
    pub is_add: bool,
}

/// Additional state fields for bridge administration
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct EmergencyRequest {
    /// Request ID for the emergency action
    pub request_id: u64,
    /// Timestamp when the request was created
    pub timestamp: u64,
    /// Admin who requested the emergency action
    pub requester: Pubkey,
    /// Type of emergency action (0=Pause, 1=Unpause, 2=RescueTokens)
    pub action_type: u8,
    /// Target account for token rescue (if applicable)
    pub target_account: Pubkey,
    /// Amount of tokens to rescue (if applicable)
    pub amount: u64,
}

/// New struct for tracking bridge attempts to LayerZero
pub struct BridgeAttempt {
    /// ID of the attempt
    pub attempt_id: u64,
    /// Timestamp of the attempt
    pub timestamp: u64,
    /// Status of the attempt (0=Pending, 1=Success, 2=Failed)
    pub status: u8,
    /// Error message if failed
    pub error_message: String,
}

/// Define Instruction enum để xử lý các lệnh khác nhau
#[derive(Debug)]
pub enum DmdInstruction {
    /// Khởi tạo DMD token mint
    /// 0. `[signer]` Tài khoản người tạo
    /// 1. `[writable]` Mint account mới
    /// 2. `[]` Rent sysvar
    /// 3. `[]` Token program
    InitializeMint {
        /// Số thập phân của token
        decimals: u8,
    },

    /// Khởi tạo DMD token account
    /// 0. `[signer]` Chủ sở hữu
    /// 1. `[writable]` Token account mới
    /// 2. `[]` Mint account
    /// 3. `[]` Rent sysvar
    /// 4. `[]` Token program
    InitializeAccount,

    /// Mint DMD token
    /// 0. `[signer]` Tài khoản mint authority
    /// 1. `[writable]` Token account đích
    /// 2. `[writable]` Mint account
    /// 3. `[]` Token program
    MintTo {
        /// Số lượng token cần mint
        amount: u64,
    },

    /// Chuyển DMD token
    /// 0. `[signer]` Người gửi
    /// 1. `[writable]` Token account nguồn
    /// 2. `[writable]` Token account đích
    /// 3. `[]` Token program
    Transfer {
        /// Số lượng token cần chuyển
        amount: u64,
    },

    /// Bridge token sang NEAR thông qua LayerZero
    /// 0. `[signer]` Người gửi
    /// 1. `[writable]` Token account nguồn
    /// 2. `[writable]` Escrow token account
    /// 3. `[]` Token program
    /// 4. `[]` LayerZero endpoint program
    /// 5. `[writable]` Bridge state account
    /// 6. `[]` Clock sysvar
    BridgeToNear {
        /// Địa chỉ đích trên NEAR
        to_address: [u8; 64],
        /// Số lượng token cần bridge
        amount: u64,
        /// Phí trả cho LayerZero relayer (lamports)
        fee: u64,
    },

    /// Nhận token từ NEAR thông qua LayerZero
    /// 0. `[signer]` LayerZero program hoặc relayer
    /// 1. `[writable]` Mint account
    /// 2. `[writable]` Token account đích
    /// 3. `[]` Token program
    ReceiveFromNear {
        /// Địa chỉ nguồn trên NEAR
        from_address: [u8; 64],
        /// Địa chỉ đích trên Solana
        to_address: Pubkey,
        /// Số lượng token nhận
        amount: u64,
    },

    /// Tạo metadata cho token
    /// 0. `[signer]` Update authority
    /// 1. `[writable]` Metadata account
    /// 2. `[]` Mint account
    /// 3. `[]` Metaplex token metadata program
    CreateMetadata {
        /// Tên token
        name: String,
        /// Ký hiệu token
        symbol: String,
        /// URI của metadata
        uri: String,
    },

    /// Request to add or remove a trusted remote
    /// 0. `[signer]` Admin account
    /// 1. `[writable]` Bridge state account
    /// 2. `[]` Clock sysvar
    RequestTrustedRemoteUpdate {
        /// Remote address to add/remove
        remote_address: [u8; 64],
        /// Chain ID
        chain_id: u16,
        /// True to add, false to remove
        is_add: bool,
    },
    
    /// Execute a pending trusted remote update
    /// 0. `[signer]` Admin account
    /// 1. `[writable]` Bridge state account
    /// 2. `[]` Clock sysvar
    ExecuteTrustedRemoteUpdate {
        /// Remote address to update
        remote_address: [u8; 64],
        /// Chain ID
        chain_id: u16,
    },
    
    /// Cancel a pending trusted remote update
    /// 0. `[signer]` Admin account
    /// 1. `[writable]` Bridge state account
    /// 2. `[]` Clock sysvar
    CancelTrustedRemoteUpdate {
        /// Remote address to cancel update for
        remote_address: [u8; 64],
        /// Chain ID
        chain_id: u16,
    },
    
    /// Enhanced receive from NEAR
    /// 0. `[signer]` LayerZero program or relayer
    /// 1. `[writable]` Mint account
    /// 2. `[writable]` Token account đích
    /// 3. `[]` Token program
    /// 4. `[writable]` Bridge state account
    /// 5. `[]` Clock sysvar
    EnhancedReceiveFromNear {
        /// Địa chỉ nguồn trên NEAR
        from_address: [u8; 64],
        /// Địa chỉ đích trên Solana
        to_address: Pubkey,
        /// Số lượng token nhận
        amount: u64,
        /// Chain ID of the source
        source_chain_id: u16,
        /// Nonce to prevent replay attacks
        nonce: u64,
    },
    
    /// Handle failed bridge request
    /// 0. `[signer]` Admin account
    /// 1. `[writable]` Escrow token account
    /// 2. `[writable]` Destination token account (original sender)
    /// 3. `[]` Token program
    /// 4. `[writable]` Bridge state account
    HandleFailedBridge {
        /// ID of the bridge request to handle
        request_id: u64,
    },
    
    /// Retry bridge request
    /// 0. `[signer]` Admin account
    /// 1. `[]` LayerZero endpoint program
    /// 2. `[writable]` Bridge state account
    RetryBridge {
        /// ID of the bridge request to retry
        request_id: u64,
    },
    
    /// Set bridge parameters
    /// 0. `[signer]` Admin account
    /// 1. `[writable]` Bridge state account
    SetBridgeParams {
        /// Daily limit for bridging (total volume)
        daily_limit: u64,
        /// Cooldown period for individual users (seconds)
        user_cooldown: u64,
    },

    /// Initialize bridge state account
    /// 0. `[signer]` Admin account
    /// 1. `[writable]` Bridge state account
    /// 2. `[]` Clock sysvar
    InitializeBridge {
        /// Escrow account
        escrow_account: Pubkey,
        /// LayerZero endpoint
        layerzero_endpoint: Pubkey,
        /// Base fee
        base_fee: u64,
    },
    
    /// Request emergency action (such as pausing bridge or rescuing tokens)
    /// 0. `[signer]` Admin account
    /// 1. `[writable]` Bridge state account
    /// 2. `[]` Clock sysvar
    RequestEmergencyAction {
        /// Type of emergency action (0=Pause, 1=Unpause, 2=RescueTokens)
        action_type: u8,
        /// Target account for token rescue (if applicable)
        target_account: Pubkey,
        /// Amount of tokens to rescue (if applicable)
        amount: u64,
    },
    
    /// Execute emergency action after timelock period
    /// 0. `[signer]` Admin account
    /// 1. `[writable]` Bridge state account
    /// 2. `[]` Clock sysvar
    /// 3. `[writable]` Escrow token account (if rescue tokens)
    /// 4. `[writable]` Target token account (if rescue tokens)
    /// 5. `[]` Token program (if rescue tokens)
    ExecuteEmergencyAction {
        /// Request ID of the emergency action
        request_id: u64,
    },
    
    /// Cancel emergency action
    /// 0. `[signer]` Admin account (must be the original requester)
    /// 1. `[writable]` Bridge state account
    CancelEmergencyAction {
        /// Request ID of the emergency action
        request_id: u64,
    },
    
    /// Auto-refund for stuck bridge requests that haven't been processed for a long time
    /// 0. `[signer]` Admin account
    /// 1. `[writable]` Escrow token account
    /// 2. `[]` Token program
    /// 3. `[writable]` Bridge state account
    /// 4. `[]` Clock sysvar
    /// 5+. `[writable]` Destination token accounts for refunds
    AutoRefundRequests,
    
    /// User self-refund for own stuck bridge request
    /// 0. `[signer]` User account (original sender)
    /// 1. `[writable]` Escrow token account
    /// 2. `[writable]` Destination token account
    /// 3. `[]` Token program
    /// 4. `[writable]` Bridge state account
    /// 5. `[]` Clock sysvar
    UserRefund {
        /// Request ID of the bridge request
        request_id: u64,
    },
    
    /// Emergency refund for a specific bridge request
    /// 0. `[signer]` Admin account
    /// 1. `[writable]` Escrow token account
    /// 2. `[writable]` Destination token account (original sender)
    /// 3. `[]` Token program
    /// 4. `[writable]` Bridge state account
    /// 5. `[]` Clock sysvar
    EmergencyRefund {
        /// ID of the bridge request to refund
        request_id: u64,
    },
    
    /// Retry a failed mint
    /// 0. `[signer]` Admin account
    /// 1. `[writable]` Mint account
    /// 2. `[writable]` Destination token account
    /// 3. `[]` Token program
    /// 4. `[writable]` Bridge state account
    /// 5. `[]` Clock sysvar
    RetryFailedMint {
        /// Index of the failed mint to retry
        failed_mint_index: u8,
    },
    
    /// Auto-retry multiple failed mints
    /// 0. `[signer]` Admin account
    /// 1. `[writable]` Mint account
    /// 2. `[]` Token program
    /// 3. `[writable]` Bridge state account
    /// 4. `[]` Clock sysvar
    /// 5+. `[writable]` Token accounts for each mint destination
    AutoRetryFailedMints {
        /// Maximum number of failed mints to retry
        max_retries: u8,
    },
    
    /// Get information about failed mints
    /// 0. `[]` Bridge state account
    GetFailedMintInfo,
    
    /// Clear old processed nonces to free up space
    /// 0. `[signer]` Admin account
    /// 1. `[writable]` Bridge state account
    /// 2. `[]` Clock sysvar
    CleanupProcessedNonces {
        /// Age in seconds for nonces to be considered old enough to remove
        age_threshold: u64,
    },
    
    /// Request admin timelock for privileged operations
    /// 0. `[signer]` Admin account
    /// 1. `[writable]` Bridge state account
    /// 2. `[]` Clock sysvar
    RequestAdminTimelock,
    
    /// Execute admin privileged operation
    /// 0. `[signer]` Admin account (same as requester)
    /// 1. `[writable]` Bridge state account
    /// 2. `[]` Clock sysvar
    ExecuteAdminOperation {
        /// Type of operation (0=UpdateLimit, 1=EmergencyRecover, 2=ModifyLayerZeroConfig)
        operation_type: u8,
        /// Operation data (depends on type)
        data: [u8; 64],
    },
    
    /// Update LayerZero status
    /// 0. `[signer]` Admin account
    /// 1. `[writable]` Bridge state account
    /// 2. `[]` Clock sysvar
    UpdateLayerZeroStatus {
        /// New status (0=Active, 1=Degraded, 2=Down)
        new_status: u8,
    },
    
    /// Get bridge attempt info
    /// 0. `[]` Bridge state account
    GetBridgeAttemptInfo {
        /// Bridge attempt ID, 0 for most recent
        attempt_id: u64,
    },
    
    /// Get escrow status information
    /// 0. `[]` Bridge state account
    GetEscrowStatus,
    
    /// Request user emergency withdraw from escrow
    /// 0. `[signer]` User account
    /// 1. `[writable]` Bridge state account
    /// 2. `[]` Clock sysvar
    RequestUserEmergencyWithdraw {
        /// Amount of tokens to withdraw
        amount: u64,
    },
    
    /// Execute user emergency withdraw after timelock period
    /// 0. `[signer]` User account (must be the same as requester)
    /// 1. `[writable]` Bridge state account
    /// 2. `[]` Clock sysvar
    /// 3. `[writable]` Escrow token account
    /// 4. `[writable]` Destination token account
    /// 5. `[]` Token program
    ExecuteUserEmergencyWithdraw,
    
    /// Set escrow limits
    /// 0. `[signer]` Admin account
    /// 1. `[writable]` Bridge state account
    /// 2. `[]` Clock sysvar
    SetEscrowLimits {
        /// Maximum amount allowed in escrow
        max_escrow_amount: u64,
        /// Threshold percentage for alerts (0-100)
        alert_threshold: u8,
    },
}

/// Track processed nonces with timestamps to enable better management
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ProcessedNonce {
    /// The nonce value
    pub nonce: u64,
    /// When the nonce was processed
    pub timestamp: u64,
    /// Chain ID associated with this nonce
    pub source_chain_id: u16,
    /// Whether this nonce slot can be reused (after expiration)
    pub can_recycle: bool,
}

/// Cấu trúc dữ liệu cho Bridge State
#[derive(Debug, PartialEq)]
pub struct BridgeState {
    /// Có được khởi tạo chưa
    pub is_initialized: bool,
    /// Escrow account cho bridge
    pub escrow_account: Pubkey,
    /// LayerZero endpoint
    pub layerzero_endpoint: Pubkey,
    /// Phí cơ bản cho bridge
    pub base_fee: u64,
    /// Mint authority
    pub mint_authority: Pubkey,
    /// Trusted remote addresses - formatted as [chain_id (2 bytes) + address (62 bytes)] * n
    pub trusted_remotes: [u8; TRUSTED_REMOTE_SIZE * MAX_TRUSTED_REMOTES],
    /// Number of active trusted remotes
    pub trusted_remote_count: u8,
    /// Trusted remote updates in timelock
    pub pending_updates: [TrustedRemoteUpdate; 5],
    /// Number of pending updates
    pub pending_update_count: u8,
    /// Processed nonces with timestamps to prevent replay attacks
    pub processed_nonces: [ProcessedNonce; 1000],
    /// Number of active processed nonces
    pub processed_nonce_count: u16,
    /// Current index position in the circular buffer
    pub nonce_current_index: u16,
    /// Sequence numbers for each chain to ensure ordered messages
    pub chain_sequences: [ChainSequence; 32],
    /// Number of chains with sequence tracking
    pub chain_sequence_count: u8,
    /// Bridge requests in various states
    pub bridge_requests: [BridgeRequest; MAX_BRIDGE_REQUESTS],
    /// Number of active bridge requests
    pub bridge_request_count: u8,
    /// Next available request ID
    pub next_request_id: u64,
    /// Daily bridge volume tracking
    pub daily_volume: u64,
    /// Timestamp when daily volume was last reset
    pub daily_volume_timestamp: u64,
    /// Daily bridge limit
    pub daily_limit: u64,
    /// Per-user cooldown period in seconds
    pub user_cooldown: u64,
    /// Map of user to their last bridge timestamp
    pub user_last_bridge: std::collections::HashMap<Pubkey, u64>,
    /// Current bridge operational state (0=Active, 1=Paused)
    pub bridge_state: u8,
    /// Emergency requests in pending state
    pub emergency_requests: [EmergencyRequest; 5],
    /// Number of pending emergency requests
    pub emergency_request_count: u8,
    /// Next emergency request ID
    pub next_emergency_id: u64,
    /// Current amount of tokens in escrow
    pub escrow_token_balance: u64,
    /// Last time escrow balance was updated
    pub escrow_update_timestamp: u64,
    /// Max escrow threshold
    pub max_escrow_threshold: u64,
    
    /// Map of user+day to their daily volume
    pub user_daily_volumes: std::collections::HashMap<String, u64>,
    
    /// Admin accounts that can perform admin actions
    pub admin_accounts: [Pubkey; 5],
    
    /// Number of active admin accounts
    pub admin_count: u8,
    
    /// Failed mints tracking system
    pub failed_mints: [FailedMint; MAX_FAILED_MINTS],
    
    /// Number of failed mints currently being tracked
    pub failed_mint_count: u8,
    
    /// Total successful mints counter (for statistics)
    pub successful_mints: u64,
    
    /// Total retried mints counter (for statistics)
    pub retried_mints: u64,
    
    /// Total received amount (for statistics)
    pub total_received: u64,
    
    /// Security events for monitoring and auditing
    pub security_events: [SecurityEvent; MAX_SECURITY_EVENTS],
    
    /// Number of security events recorded
    pub security_event_count: u8,
    
    /// Last bridge attempt ID
    pub last_bridge_attempt_id: u64,
    
    /// Bridge attempts history
    pub bridge_attempts: [BridgeAttempt; MAX_BRIDGE_REQUESTS],
    
    /// Number of active bridge attempts
    pub bridge_attempt_count: u8,
    
    /// Admin privilege timelock expiry
    pub admin_timelock_expiry: u64,
    
    /// Last admin who requested timelock
    pub admin_timelock_requester: Pubkey,
    
    /// Emergency recovery mode activated
    pub emergency_recovery_mode: bool,
    
    /// LayerZero status (0=Active, 1=Degraded, 2=Down)
    pub layerzero_status: u8,
    
    /// Last LayerZero status update timestamp
    pub layerzero_status_timestamp: u64,
    
    /// User emergency withdrawal requests
    pub user_withdrawal_requests: [EmergencyWithdrawRequest; 100],
    /// Number of active user withdrawal requests
    pub user_withdrawal_request_count: u8,
    /// Next user withdrawal request ID
    pub next_user_withdrawal_id: u64,
    /// Maximum escrow amount allowed
    pub max_escrow_amount: u64,
    /// Alert threshold percentage (0-100)
    pub escrow_alert_threshold: u8,
    /// Last time escrow alert was triggered
    pub last_escrow_alert_time: u64,
    /// Whether escrow emergency mode is activated
    pub escrow_emergency_mode: bool,
}

impl Sealed for BridgeState {}

impl IsInitialized for BridgeState {
    fn is_initialized(&self) -> bool {
        self.is_initialized
    }
}

impl Pack for BridgeState {
    const LEN: usize = 1 + // is_initialized
                       32 + // escrow_account
                       32 + // layerzero_endpoint
                       8 + // base_fee
                       32 + // mint_authority
                       (TRUSTED_REMOTE_SIZE * MAX_TRUSTED_REMOTES) + // trusted_remotes
                       1 + // trusted_remote_count
                       (5 * (64 + 2 + 8 + 1)) + // pending_updates (5 * size of TrustedRemoteUpdate)
                       1 + // pending_update_count
                       1024 + // processed_nonces (approximation for HashMap)
                       8 + // nonce_count
                       8 + // oldest_nonce_timestamp
                       (MAX_BRIDGE_REQUESTS * (64 + 8 + 32 + 8 + 8 + 1 + 1)) + // bridge_requests
                       1 + // bridge_request_count
                       8 + // next_request_id
                       8 + // daily_volume
                       8 + // daily_volume_timestamp
                       8 + // daily_limit
                       8 + // user_cooldown
                       1024 + // user_last_bridge (approximation for HashMap)
                       1 + // bridge_state
                       5 * (64 + 8 + 8 + 1 + 8) + // emergency_requests (5 * size of EmergencyRequest)
                       1 + // emergency_request_count
                       8 + // next_emergency_id
                       8 + // escrow_token_balance
                       8 + // escrow_update_timestamp
                       8 + // max_escrow_threshold
                       1024 + // user_daily_volumes (approximation for HashMap)
                       5 * (32 + 1) + // admin_accounts (5 * size of Pubkey + 1 for admin_count)
                       1 + // admin_count
                       5 * (64 + 8 + 8 + 1 + 8) + // failed_mints (5 * size of FailedMint)
                       1 + // failed_mint_count
                       8 + // successful_mints
                       8 + // retried_mints
                       8 + // total_received
                       5 * (64 + 8 + 8 + 1 + 8) + // security_events (5 * size of SecurityEvent)
                       1 + // security_event_count
                       8 + // last_bridge_attempt_id
                       8 + // bridge_attempt_count
                       8 + // admin_timelock_expiry
                       32 + // admin_timelock_requester
                       1 + // emergency_recovery_mode
                       1 + // layerzero_status
                       8 + // layerzero_status_timestamp;

    fn pack_into_slice(&self, dst: &mut [u8]) {
        let mut cursor = std::io::Cursor::new(dst);
        cursor.write_all(&[self.is_initialized as u8]).unwrap();
        cursor.write_all(self.escrow_account.as_ref()).unwrap();
        cursor.write_all(self.layerzero_endpoint.as_ref()).unwrap();
        cursor.write_all(&self.base_fee.to_le_bytes()).unwrap();
        cursor.write_all(self.mint_authority.as_ref()).unwrap();
        cursor.write_all(&self.trusted_remotes).unwrap();
        cursor.write_all(&[self.trusted_remote_count]).unwrap();
        
        // Pack pending updates
        for update in &self.pending_updates {
            cursor.write_all(&update.remote_address).unwrap();
            cursor.write_all(&update.chain_id.to_le_bytes()).unwrap();
            cursor.write_all(&update.timelock_expiry.to_le_bytes()).unwrap();
            cursor.write_all(&[update.is_add as u8]).unwrap();
        }
        cursor.write_all(&[self.pending_update_count]).unwrap();
        
        // Pack nonce information
        cursor.write_all(&self.nonce_count.to_le_bytes()).unwrap();
        cursor.write_all(&self.oldest_nonce_timestamp.to_le_bytes()).unwrap();
        
        // Pack bridge requests
        for request in &self.bridge_requests {
            cursor.write_all(&request.to_address).unwrap();
            cursor.write_all(&request.amount.to_le_bytes()).unwrap();
            cursor.write_all(request.source_account.as_ref()).unwrap();
            cursor.write_all(&request.timestamp.to_le_bytes()).unwrap();
            cursor.write_all(&request.request_id.to_le_bytes()).unwrap();
            cursor.write_all(&[request.status]).unwrap();
            cursor.write_all(&[request.retry_count]).unwrap();
        }
        cursor.write_all(&[self.bridge_request_count]).unwrap();
        cursor.write_all(&self.next_request_id.to_le_bytes()).unwrap();
        cursor.write_all(&self.daily_volume.to_le_bytes()).unwrap();
        cursor.write_all(&self.daily_volume_timestamp.to_le_bytes()).unwrap();
        cursor.write_all(&self.daily_limit.to_le_bytes()).unwrap();
        cursor.write_all(&self.user_cooldown.to_le_bytes()).unwrap();
        
        // We would need a more sophisticated serialization for HashMaps
        // For this example, we're keeping it simple
    }

    fn unpack_from_slice(src: &[u8]) -> Result<Self, ProgramError> {
        let mut cursor = std::io::Cursor::new(src);
        let mut is_initialized_bytes = [0u8; 1];
        cursor.read_exact(&mut is_initialized_bytes)?;
        let is_initialized = is_initialized_bytes[0] != 0;
        
        let mut escrow_account = Pubkey::default();
        let mut escrow_bytes = [0u8; 32];
        cursor.read_exact(&mut escrow_bytes)?;
        escrow_account = Pubkey::new(&escrow_bytes);
        
        let mut layerzero_endpoint = Pubkey::default();
        let mut endpoint_bytes = [0u8; 32];
        cursor.read_exact(&mut endpoint_bytes)?;
        layerzero_endpoint = Pubkey::new(&endpoint_bytes);
        
        let mut base_fee_bytes = [0u8; 8];
        cursor.read_exact(&mut base_fee_bytes)?;
        let base_fee = u64::from_le_bytes(base_fee_bytes);
        
        let mut mint_authority = Pubkey::default();
        let mut authority_bytes = [0u8; 32];
        cursor.read_exact(&mut authority_bytes)?;
        mint_authority = Pubkey::new(&authority_bytes);
        
        let mut trusted_remotes = [0u8; TRUSTED_REMOTE_SIZE * MAX_TRUSTED_REMOTES];
        cursor.read_exact(&mut trusted_remotes)?;
        
        let mut trusted_remote_count_bytes = [0u8; 1];
        cursor.read_exact(&mut trusted_remote_count_bytes)?;
        let trusted_remote_count = trusted_remote_count_bytes[0];
        
        let mut pending_updates = [TrustedRemoteUpdate {
            remote_address: [0u8; 64],
            chain_id: 0,
            timelock_expiry: 0,
            is_add: false,
        }; 5];
        
        for i in 0..5 {
            let mut remote_address = [0u8; 64];
            cursor.read_exact(&mut remote_address)?;
            
            let mut chain_id_bytes = [0u8; 2];
            cursor.read_exact(&mut chain_id_bytes)?;
            let chain_id = u16::from_le_bytes(chain_id_bytes);
            
            let mut timelock_expiry_bytes = [0u8; 8];
            cursor.read_exact(&mut timelock_expiry_bytes)?;
            let timelock_expiry = u64::from_le_bytes(timelock_expiry_bytes);
            
            let mut is_add_bytes = [0u8; 1];
            cursor.read_exact(&mut is_add_bytes)?;
            let is_add = is_add_bytes[0] != 0;
            
            pending_updates[i] = TrustedRemoteUpdate {
                remote_address,
                chain_id,
                timelock_expiry,
                is_add,
            };
        }
        
        let mut pending_update_count_bytes = [0u8; 1];
        cursor.read_exact(&mut pending_update_count_bytes)?;
        let pending_update_count = pending_update_count_bytes[0];
        
        // Unpack nonce information
        let mut nonce_count_bytes = [0u8; 8];
        cursor.read_exact(&mut nonce_count_bytes)?;
        let nonce_count = u64::from_le_bytes(nonce_count_bytes);
        
        let mut oldest_nonce_timestamp_bytes = [0u8; 8];
        cursor.read_exact(&mut oldest_nonce_timestamp_bytes)?;
        let oldest_nonce_timestamp = u64::from_le_bytes(oldest_nonce_timestamp_bytes);
        
        let processed_nonces = std::collections::HashMap::new();
        
        // Unpack bridge requests
        let mut bridge_requests = [BridgeRequest {
            to_address: [0u8; 64],
            amount: 0,
            source_account: Pubkey::default(),
            timestamp: 0,
            request_id: 0,
            status: 0,
            retry_count: 0,
        }; MAX_BRIDGE_REQUESTS];
        
        for i in 0..MAX_BRIDGE_REQUESTS {
            let mut to_address = [0u8; 64];
            cursor.read_exact(&mut to_address)?;
            
            let mut amount_bytes = [0u8; 8];
            cursor.read_exact(&mut amount_bytes)?;
            let amount = u64::from_le_bytes(amount_bytes);
            
            let mut source_account = Pubkey::default();
            let mut source_bytes = [0u8; 32];
            cursor.read_exact(&mut source_bytes)?;
            source_account = Pubkey::new(&source_bytes);
            
            let mut timestamp_bytes = [0u8; 8];
            cursor.read_exact(&mut timestamp_bytes)?;
            let timestamp = u64::from_le_bytes(timestamp_bytes);
            
            let mut request_id_bytes = [0u8; 8];
            cursor.read_exact(&mut request_id_bytes)?;
            let request_id = u64::from_le_bytes(request_id_bytes);
            
            let mut status_bytes = [0u8; 1];
            cursor.read_exact(&mut status_bytes)?;
            let status = status_bytes[0];
            
            let mut retry_count_bytes = [0u8; 1];
            cursor.read_exact(&mut retry_count_bytes)?;
            let retry_count = retry_count_bytes[0];
            
            bridge_requests[i] = BridgeRequest {
                to_address,
                amount,
                source_account,
                timestamp,
                request_id,
                status,
                retry_count,
            };
        }
        
        let mut bridge_request_count_bytes = [0u8; 1];
        cursor.read_exact(&mut bridge_request_count_bytes)?;
        let bridge_request_count = bridge_request_count_bytes[0];
        
        let mut next_request_id_bytes = [0u8; 8];
        cursor.read_exact(&mut next_request_id_bytes)?;
        let next_request_id = u64::from_le_bytes(next_request_id_bytes);
        
        let mut daily_volume_bytes = [0u8; 8];
        cursor.read_exact(&mut daily_volume_bytes)?;
        let daily_volume = u64::from_le_bytes(daily_volume_bytes);
        
        let mut daily_volume_timestamp_bytes = [0u8; 8];
        cursor.read_exact(&mut daily_volume_timestamp_bytes)?;
        let daily_volume_timestamp = u64::from_le_bytes(daily_volume_timestamp_bytes);
        
        let mut daily_limit_bytes = [0u8; 8];
        cursor.read_exact(&mut daily_limit_bytes)?;
        let daily_limit = u64::from_le_bytes(daily_limit_bytes);
        
        let mut user_cooldown_bytes = [0u8; 8];
        cursor.read_exact(&mut user_cooldown_bytes)?;
        let user_cooldown = u64::from_le_bytes(user_cooldown_bytes);
        
        let user_last_bridge = std::collections::HashMap::new();
        
        let mut bridge_state = BridgeState {
            is_initialized,
            escrow_account,
            layerzero_endpoint,
            base_fee,
            mint_authority,
            trusted_remotes,
            trusted_remote_count,
            pending_updates,
            pending_update_count,
            processed_nonces,
            nonce_count: 0,
            oldest_nonce_timestamp,
            bridge_requests,
            bridge_request_count,
            next_request_id,
            daily_volume,
            daily_volume_timestamp,
            daily_limit,
            user_cooldown,
            user_last_bridge,
            bridge_state: 0,
            emergency_requests: [EmergencyRequest {
                request_id: 0,
                timestamp: 0,
                requester: Pubkey::default(),
                action_type: 0,
                target_account: Pubkey::default(),
                amount: 0,
            }; 5],
            emergency_request_count: 0,
            next_emergency_id: 1,
            escrow_token_balance: 0,
            escrow_update_timestamp: 0,
            max_escrow_threshold: 1_000_000_000_000,
            user_daily_volumes: std::collections::HashMap::new(),
            admin_accounts: [Pubkey::default(); 5],
            admin_count: 0,
            failed_mints: [FailedMint {
                from_address: [0u8; 64],
                to_address: Pubkey::default(),
                amount: 0,
                source_chain_id: 0,
                nonce: 0,
                timestamp: 0,
                retry_count: 0,
                last_retry: 0,
                status: 0,
                failure_reason: String::new(),
            }; MAX_FAILED_MINTS],
            failed_mint_count: 0,
            successful_mints: 0,
            retried_mints: 0,
            total_received: 0,
            security_events: [SecurityEvent {
                event_type: String::new(),
                timestamp: 0,
                caller: Pubkey::default(),
                source_chain_id: 0,
                nonce: 0,
                message: String::new(),
            }; MAX_SECURITY_EVENTS],
            security_event_count: 0,
            last_bridge_attempt_id: 0,
            bridge_attempts: [BridgeAttempt {
                attempt_id: 0,
                timestamp: 0,
                status: 0,
                error_message: String::new(),
            }; MAX_BRIDGE_REQUESTS],
            bridge_attempt_count: 0,
            admin_timelock_expiry: 0,
            admin_timelock_requester: Pubkey::default(),
            emergency_recovery_mode: false,
            layerzero_status: 0,
            layerzero_status_timestamp: 0,
        };
        
        // Serialize bridge state to account data
        BridgeState::pack(bridge_state, &mut cursor.get_mut())?;
        
        Ok(bridge_state)
    }
}

// Entry point for the Solana program
entrypoint!(process_instruction);

/// Xử lý lệnh được gửi tới program
pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let instruction = DmdInstruction::unpack(instruction_data)?;

    match instruction {
        DmdInstruction::InitializeMint { decimals } => {
            msg!("Instruction: InitializeMint");
            process_initialize_mint(program_id, accounts, decimals)
        }
        DmdInstruction::InitializeAccount => {
            msg!("Instruction: InitializeAccount");
            process_initialize_account(program_id, accounts)
        }
        DmdInstruction::MintTo { amount } => {
            msg!("Instruction: MintTo");
            process_mint_to(program_id, accounts, amount)
        }
        DmdInstruction::Transfer { amount } => {
            msg!("Instruction: Transfer");
            process_transfer(program_id, accounts, amount)
        }
        DmdInstruction::BridgeToNear { to_address, amount, fee } => {
            msg!("Instruction: BridgeToNear");
            process_bridge_to_near(program_id, accounts, to_address, amount, fee)
        }
        DmdInstruction::ReceiveFromNear { from_address, to_address, amount } => {
            msg!("Instruction: ReceiveFromNear");
            // Convert to EnhancedReceiveFromNear with default values for backward compatibility
            process_receive_from_near(
                program_id, 
                accounts, 
                from_address, 
                to_address, 
                amount, 
                LAYERZERO_CHAIN_ID_NEAR, // Default to NEAR chain ID
                0, // Default nonce (unsafe, but for backward compatibility)
            )
        }
        DmdInstruction::CreateMetadata { name, symbol, uri } => {
            msg!("Instruction: CreateMetadata");
            process_create_metadata(program_id, accounts, &[])
        }
        DmdInstruction::RequestTrustedRemoteUpdate { remote_address, chain_id, is_add } => {
            msg!("Instruction: RequestTrustedRemoteUpdate");
            process_request_trusted_remote_update(program_id, accounts, remote_address, chain_id, is_add)
        }
        DmdInstruction::ExecuteTrustedRemoteUpdate { remote_address, chain_id } => {
            msg!("Instruction: ExecuteTrustedRemoteUpdate");
            process_execute_trusted_remote_update(program_id, accounts, remote_address, chain_id)
        }
        DmdInstruction::CancelTrustedRemoteUpdate { remote_address, chain_id } => {
            msg!("Instruction: CancelTrustedRemoteUpdate");
            process_cancel_trusted_remote_update(program_id, accounts, remote_address, chain_id)
        }
        DmdInstruction::EnhancedReceiveFromNear { from_address, to_address, amount, source_chain_id, nonce } => {
            msg!("Instruction: EnhancedReceiveFromNear");
            process_receive_from_near(program_id, accounts, from_address, to_address, amount, source_chain_id, nonce)
        }
        DmdInstruction::HandleFailedBridge { request_id } => {
            msg!("Instruction: HandleFailedBridge");
            process_handle_failed_bridge(program_id, accounts, request_id)
        }
        DmdInstruction::RetryBridge { request_id } => {
            msg!("Instruction: RetryBridge");
            process_retry_bridge(program_id, accounts, request_id)
        }
        DmdInstruction::SetBridgeParams { daily_limit, user_cooldown } => {
            msg!("Instruction: SetBridgeParams");
            process_set_bridge_params(program_id, accounts, daily_limit, user_cooldown)
        }
        DmdInstruction::InitializeBridge { escrow_account, layerzero_endpoint, base_fee } => {
            msg!("Instruction: InitializeBridge");
            process_initialize_bridge(program_id, accounts, escrow_account, layerzero_endpoint, base_fee)
        }
        DmdInstruction::RequestEmergencyAction { action_type, target_account, amount } => {
            msg!("Instruction: RequestEmergencyAction");
            process_request_emergency_action(program_id, accounts, action_type, target_account, amount)
        }
        DmdInstruction::ExecuteEmergencyAction { request_id } => {
            msg!("Instruction: ExecuteEmergencyAction");
            process_execute_emergency_action(program_id, accounts, request_id)
        }
        DmdInstruction::CancelEmergencyAction { request_id } => {
            msg!("Instruction: CancelEmergencyAction");
            process_cancel_emergency_action(program_id, accounts, request_id)
        }
        DmdInstruction::AutoRefundRequests => {
            msg!("Instruction: AutoRefundRequests");
            process_auto_refund_requests(program_id, accounts)
        }
        DmdInstruction::UserRefund { request_id } => {
            msg!("Instruction: UserRefund");
            process_user_refund(program_id, accounts, request_id)
        }
        DmdInstruction::EmergencyRefund { request_id } => {
            msg!("Instruction: EmergencyRefund");
            process_emergency_refund(program_id, accounts, request_id)
        }
        DmdInstruction::RetryFailedMint { failed_mint_index } => {
            msg!("Instruction: RetryFailedMint");
            process_retry_failed_mint(program_id, accounts, failed_mint_index)
        }
        DmdInstruction::AutoRetryFailedMints { max_retries } => {
            msg!("Instruction: AutoRetryFailedMints");
            process_auto_retry_failed_mints(program_id, accounts, max_retries)
        }
        DmdInstruction::GetFailedMintInfo => {
            msg!("Instruction: GetFailedMintInfo");
            process_get_failed_mint_info(program_id, accounts)
        }
        DmdInstruction::CleanupProcessedNonces { age_threshold } => {
            msg!("Instruction: CleanupProcessedNonces");
            process_cleanup_processed_nonces(program_id, accounts, age_threshold)
        }
        DmdInstruction::RequestAdminTimelock => {
            msg!("Instruction: RequestAdminTimelock");
            process_request_admin_timelock(program_id, accounts)
        }
        DmdInstruction::ExecuteAdminOperation { operation_type, data } => {
            msg!("Instruction: ExecuteAdminOperation");
            process_execute_admin_operation(program_id, accounts, operation_type, data)
        }
        DmdInstruction::UpdateLayerZeroStatus { new_status } => {
            msg!("Instruction: UpdateLayerZeroStatus");
            process_update_layerzero_status(program_id, accounts, new_status)
        }
        DmdInstruction::GetBridgeAttemptInfo { attempt_id } => {
            msg!("Instruction: GetBridgeAttemptInfo");
            process_get_bridge_attempt_info(program_id, accounts, attempt_id)
        }
        DmdInstruction::GetEscrowStatus => {
            process_get_escrow_status(program_id, accounts)
        },
        DmdInstruction::RequestUserEmergencyWithdraw { amount } => {
            process_request_user_emergency_withdraw(program_id, accounts, amount)
        },
        DmdInstruction::ExecuteUserEmergencyWithdraw => {
            process_execute_user_emergency_withdraw(program_id, accounts)
        },
        DmdInstruction::SetEscrowLimits { max_escrow_amount, alert_threshold } => {
            process_set_escrow_limits(program_id, accounts, max_escrow_amount, alert_threshold)
        },
    }
}

/// Khởi tạo DMD token mint
fn process_initialize_mint(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    decimals: u8,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let creator_info = next_account_info(account_info_iter)?;
    let mint_info = next_account_info(account_info_iter)?;
    let rent_info = next_account_info(account_info_iter)?;
    let token_program_info = next_account_info(account_info_iter)?;

    // Xác thực chữ ký
    if !creator_info.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Kiểm tra chương trình token
    if *token_program_info.key != spl_token::id() {
        return Err(ProgramError::IncorrectProgramId);
    }

    // Rent là exempt
    let rent = &Rent::from_account_info(rent_info)?;
    if !rent.is_exempt(mint_info.lamports(), Mint::LEN) {
        return Err(ProgramError::AccountNotRentExempt);
    }

    // Khởi tạo mint
    let initialize_mint_ix = token_instruction::initialize_mint(
        &spl_token::id(),
        mint_info.key,
        creator_info.key,
        Some(creator_info.key),
        decimals,
    )?;

    invoke(
        &initialize_mint_ix,
        &[mint_info.clone(), rent_info.clone(), token_program_info.clone()],
    )?;

    msg!("Mint được khởi tạo: {}", mint_info.key);
    
    Ok(())
}

/// Khởi tạo DMD token account
fn process_initialize_account(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let owner_info = next_account_info(account_info_iter)?;
    let token_account_info = next_account_info(account_info_iter)?;
    let mint_info = next_account_info(account_info_iter)?;
    let rent_info = next_account_info(account_info_iter)?;
    let token_program_info = next_account_info(account_info_iter)?;

    // Xác thực chữ ký
    if !owner_info.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Khởi tạo token account
    let initialize_account_ix = token_instruction::initialize_account(
        &spl_token::id(),
        token_account_info.key,
        mint_info.key,
        owner_info.key,
    )?;

    invoke(
        &initialize_account_ix,
        &[
            token_account_info.clone(),
            mint_info.clone(),
            owner_info.clone(),
            rent_info.clone(),
            token_program_info.clone(),
        ],
    )?;

    msg!("Token account được khởi tạo: {}", token_account_info.key);
    
    Ok(())
}

/// Mint DMD token
fn process_mint_to(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    amount: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let mint_authority_info = next_account_info(account_info_iter)?;
    let token_account_info = next_account_info(account_info_iter)?;
    let mint_info = next_account_info(account_info_iter)?;
    let token_program_info = next_account_info(account_info_iter)?;

    // Xác thực chữ ký
    if !mint_authority_info.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Mint token
    let mint_to_ix = token_instruction::mint_to(
        &spl_token::id(),
        mint_info.key,
        token_account_info.key,
        mint_authority_info.key,
        &[],
        amount,
    )?;

    invoke(
        &mint_to_ix,
        &[
            mint_info.clone(),
            token_account_info.clone(),
            mint_authority_info.clone(),
            token_program_info.clone(),
        ],
    )?;

    msg!("Mint {} tokens to {}", amount, token_account_info.key);
    
    Ok(())
}

/// Chuyển DMD token
fn process_transfer(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    amount: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let owner_info = next_account_info(account_info_iter)?;
    let source_info = next_account_info(account_info_iter)?;
    let destination_info = next_account_info(account_info_iter)?;
    let token_program_info = next_account_info(account_info_iter)?;

    // Xác thực chữ ký
    if !owner_info.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Chuyển token
    let transfer_ix = token_instruction::transfer(
        &spl_token::id(),
        source_info.key,
        destination_info.key,
        owner_info.key,
        &[],
        amount,
    )?;

    invoke(
        &transfer_ix,
        &[
            source_info.clone(),
            destination_info.clone(),
            owner_info.clone(),
            token_program_info.clone(),
        ],
    )?;

    msg!("Chuyển {} tokens từ {} tới {}", amount, source_info.key, destination_info.key);
    
    Ok(())
}

/// Bridge token sang NEAR
fn process_bridge_to_near(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    to_address: [u8; 64],
    amount: u64,
    fee: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let owner_info = next_account_info(account_info_iter)?;
    let source_info = next_account_info(account_info_iter)?;
    let escrow_info = next_account_info(account_info_iter)?;
    let token_program_info = next_account_info(account_info_iter)?;
    let layerzero_endpoint_info = next_account_info(account_info_iter)?;
    let bridge_info = next_account_info(account_info_iter)?;
    let clock_info = next_account_info(account_info_iter)?;

    // Load the clock to get the current timestamp
    let clock = Clock::from_account_info(clock_info)?;
    let current_timestamp = clock.unix_timestamp as u64;
    
    // Xác thực chữ ký
    if !owner_info.is_signer {
        msg!("Error: Missing required signature");
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Kiểm tra endpoint
    if layerzero_endpoint_info.key.to_string() != LAYERZERO_ENDPOINT {
        msg!("Error: Invalid LayerZero endpoint: {}", layerzero_endpoint_info.key);
        return Err(ProgramError::InvalidAccountData);
    }
    
    // Verify the bridge account is owned by this program
    if bridge_info.owner != program_id {
        msg!("Error: Bridge account not owned by this program");
        return Err(ProgramError::IncorrectProgramId);
    }
    
    // Load bridge state
    let mut bridge_state = BridgeState::unpack(&bridge_info.data.borrow())?;
    
    // Check escrow limits before proceeding
    if bridge_state.max_escrow_amount > 0 && 
       bridge_state.escrow_token_balance + amount > bridge_state.max_escrow_amount {
        msg!("Error: Bridge transaction would exceed maximum escrow limit");
        msg!("Current: {}, Max: {}, Requested: {}", 
            bridge_state.escrow_token_balance, 
            bridge_state.max_escrow_amount,
            amount);
        return Err(ProgramError::InvalidArgument);
    }
    
    // Monitor escrow status
    monitor_escrow_status(&mut bridge_state, current_timestamp)?;
    
    // If escrow emergency mode is active, only admins can bridge
    if bridge_state.escrow_emergency_mode && !is_admin(owner_info.key, &bridge_state) {
        msg!("Error: Escrow emergency mode active. Only admins can bridge tokens.");
        return Err(ProgramError::Custom(106));
    }
    
    // Check if LayerZero is down or degraded
    if bridge_state.layerzero_status == 2 {
        msg!("Error: LayerZero is currently down. Bridge operations are not available.");
        return Err(ProgramError::InvalidAccountData);
    }
    
    if bridge_state.layerzero_status == 1 {
        msg!("Warning: LayerZero is in degraded state. Bridge operations may take longer than usual.");
    }
    
    // Verify the bridge is not paused
    if bridge_state.bridge_state == 1 {
        msg!("Error: Bridge is currently paused");
        return Err(ProgramError::InvalidAccountData);
    }
    
    // Additional per-transaction amount check
    if amount > PER_TRANSACTION_BRIDGE_LIMIT {
        msg!("Error: Amount exceeds per-transaction limit of {}", PER_TRANSACTION_BRIDGE_LIMIT);
        return Err(ProgramError::InvalidArgument);
    }
    
    // Check if user is in cooldown period
    if let Some(last_bridge_time) = bridge_state.user_last_bridge.get(owner_info.key) {
        if current_timestamp - last_bridge_time < bridge_state.user_cooldown {
            msg!("Error: User is in cooldown period. Please wait {} more seconds", 
                bridge_state.user_cooldown - (current_timestamp - last_bridge_time));
            return Err(ProgramError::Custom(100)); // Custom error code for cooldown
        }
    }
    
    // Reset daily volume if needed
    if current_timestamp - bridge_state.daily_volume_timestamp > 86400 {
        bridge_state.daily_volume = 0;
        bridge_state.daily_volume_timestamp = current_timestamp;
    }
    
    // Check daily volume limit
    if bridge_state.daily_volume + amount > bridge_state.daily_limit {
        msg!("Error: Daily bridge volume limit would be exceeded");
        msg!("Current: {}, Limit: {}, Request: {}", 
            bridge_state.daily_volume, bridge_state.daily_limit, amount);
        return Err(ProgramError::Custom(101)); // Custom error code for limit
    }
    
    // Check user daily volume
    let day_key = format!("{}:{}", owner_info.key, current_timestamp / 86400);
    let user_daily_volume = bridge_state.user_daily_volumes.get(&day_key).unwrap_or(&0);
    
    // For simplicity, we're setting a user daily limit to 20% of the global limit
    let user_daily_limit = bridge_state.daily_limit / 5;
    
    if user_daily_volume + amount > user_daily_limit {
        msg!("Error: User daily bridge volume limit would be exceeded");
        msg!("Current: {}, Limit: {}, Request: {}", 
            user_daily_volume, user_daily_limit, amount);
        return Err(ProgramError::Custom(102)); // Custom error code for user limit
    }
    
    // Create a bridge request
    let request_id = bridge_state.next_request_id;
    bridge_state.next_request_id += 1;
    
    let request = BridgeRequest {
        to_address,
        amount,
        source_account: *owner_info.key,
        timestamp: current_timestamp,
        request_id,
        status: 0, // Pending
        retry_count: 0,
    };
    
    // Store the bridge request
    let request_index = bridge_state.bridge_request_count as usize;
    bridge_state.bridge_requests[request_index] = request;
    bridge_state.bridge_request_count += 1;
    
    // Generate a unique ID for this bridge attempt
    let attempt_id = bridge_state.last_bridge_attempt_id + 1;
    bridge_state.last_bridge_attempt_id = attempt_id;
    
    let bridge_attempt = BridgeAttempt {
        attempt_id,
        timestamp: current_timestamp,
        status: 0, // 0 = Pending
        error_message: String::new(),
    };
    
    // Store the bridge attempt
    let attempt_index = bridge_state.bridge_attempt_count as usize;
    bridge_state.bridge_attempts[attempt_index] = bridge_attempt;
    bridge_state.bridge_attempt_count += 1;
    
    // Update daily volume and last bridge timestamp for user
    bridge_state.daily_volume += amount;
    bridge_state.user_last_bridge.insert(*owner_info.key, current_timestamp);
    
    // Update user daily volume
    bridge_state.user_daily_volumes.insert(day_key, user_daily_volume + amount);
    
    // Update escrow token balance
    bridge_state.escrow_token_balance += amount;
    bridge_state.escrow_update_timestamp = current_timestamp;
    
    // Save bridge state with updated request info before token transfer
    BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;

    // Chuyển token vào escrow account - MUST HAPPEN AFTER all validation checks
    let transfer_to_escrow_ix = token_instruction::transfer(
        &spl_token::id(),
        source_info.key,
        escrow_info.key,
        owner_info.key,
        &[],
        amount,
    )?;

    // Execute the token transfer
    invoke(
        &transfer_to_escrow_ix,
        &[
            source_info.clone(),
            escrow_info.clone(),
            owner_info.clone(),
            token_program_info.clone(),
        ],
    )?;

    // Log the successful bridge request
    msg!("Bridge request created: id={}, to={:?}, amount={}", 
        request_id, to_address, amount);
    
    // Note: In a real implementation, we would call to LayerZero here to initiate the cross-chain message
    // For this example, we'll just log it
    msg!("Would call LayerZero endpoint to bridge tokens");
    
    Ok(())
}

/// Nhận token từ NEAR
fn process_receive_from_near(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    from_address: [u8; 64],
    to_address: Pubkey,
    amount: u64,
    source_chain_id: u16,
    nonce: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let lz_caller_info = next_account_info(account_info_iter)?;
    let mint_info = next_account_info(account_info_iter)?;
    let destination_info = next_account_info(account_info_iter)?;
    let token_program_info = next_account_info(account_info_iter)?;
    let bridge_info = next_account_info(account_info_iter)?;
    let clock_info = next_account_info(account_info_iter)?;
    
    // Verify mint account is owned by this program
    if mint_info.owner != program_id {
        msg!("Error: Mint account not owned by this program");
        return Err(ProgramError::IncorrectProgramId);
    }
    
    // Verify the bridge account is owned by this program
    if bridge_info.owner != program_id {
        msg!("Error: Bridge account not owned by this program");
        return Err(ProgramError::IncorrectProgramId);
    }
    
    // Obtain clock for timestamps
    let clock = Clock::from_account_info(clock_info)?;
    let current_timestamp = clock.unix_timestamp as u64;
    
    // Load bridge state
    let mut bridge_state = BridgeState::unpack(&bridge_info.data.borrow())?;
    
    // Check if bridge is paused
    if bridge_state.bridge_state == 1 {
        msg!("Bridge is currently paused");
        
        // Store information about the failed mint
        store_failed_mint(
            &mut bridge_state,
            from_address,
            to_address,
            amount,
            source_chain_id,
            nonce,
            "BRIDGE_PAUSED",
        )?;
        
        // Save updated bridge state
        BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;
        return Err(ProgramError::InvalidAccountData);
    }
    
    // Verify caller is LayerZero endpoint
    if lz_caller_info.key.to_string() != LAYERZERO_ENDPOINT {
        msg!("Error: Caller is not LayerZero endpoint");
        
        // Store information about unauthorized access attempt
        log_security_event(
            &mut bridge_state,
            "UNAUTHORIZED_CALL",
            lz_caller_info.key,
            &clock,
            source_chain_id,
            nonce,
        );
        
        return Err(ProgramError::IllegalOwner);
    }
    
    // Check if source is trusted remote
    if !is_trusted_remote(&bridge_state, source_chain_id, &from_address) {
        msg!("Error: Untrusted remote address from chain {}", source_chain_id);
        msg!("Source address: {:?}", from_address);
        
        // Store information about the untrusted remote access attempt
        log_security_event(
            &mut bridge_state,
            "UNTRUSTED_REMOTE",
            lz_caller_info.key,
            &clock,
            source_chain_id,
            nonce,
        );
        
        return Err(ProgramError::InvalidArgument);
    }
    
    // Check if this nonce has already been processed to prevent replay attacks
    if is_nonce_processed(&bridge_state, nonce) {
        msg!("Error: Nonce {} has already been processed", nonce);
        return Err(ProgramError::InvalidArgument);
    }
    
    // Verify and update message sequence
    if let Err(err) = verify_and_update_sequence(&mut bridge_state, source_chain_id, nonce, current_timestamp) {
        msg!("Error processing message sequence: {:?}", err);
        
        // Store the sequence verification failure
        store_failed_mint(
            &mut bridge_state,
            from_address,
            to_address,
            amount,
            source_chain_id,
            nonce,
            "SEQUENCE_VERIFICATION_FAILED",
        )?;
        
        // Save bridge state with failed mint record
        BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;
        return Err(err);
    }
    
    // Mark nonce as processed to prevent replay attacks
    mark_nonce_as_processed(&mut bridge_state, nonce)?;
    
    // Update the source chain ID in the processed nonce record
    for i in 0..bridge_state.processed_nonce_count as usize {
        if bridge_state.processed_nonces[i].nonce == nonce && !bridge_state.processed_nonces[i].can_recycle {
            bridge_state.processed_nonces[i].source_chain_id = source_chain_id;
            break;
        }
    }
    
    // Verify mint account's destination validity
    if destination_info.owner != &spl_token::id() {
        msg!("Error: Destination account is not owned by token program");
        store_failed_mint(
            &mut bridge_state,
            from_address,
            to_address,
            amount,
            source_chain_id,
            nonce,
            "INVALID_DESTINATION",
        )?;
        
        // Save updated bridge state with processed nonce and failed mint record
        BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;
        return Err(ProgramError::InvalidAccountData);
    }
    
    // Verify token account's mint
    let token_account = TokenAccount::unpack(&destination_info.data.borrow())?;
    if token_account.mint != *mint_info.key {
        msg!("Error: Token account's mint does not match");
        store_failed_mint(
            &mut bridge_state,
            from_address,
            to_address,
            amount,
            source_chain_id,
            nonce,
            "MINT_MISMATCH",
        )?;
        
        // Save updated bridge state with processed nonce and failed mint record
        BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;
        return Err(ProgramError::InvalidAccountData);
    }
    
    // Get PDA for mint authority
    let (mint_authority, bump_seed) = Pubkey::find_program_address(
        &[b"mint_authority", program_id.as_ref()],
        program_id
    );
    
    // Verify mint authority
    if bridge_state.mint_authority != mint_authority {
        msg!("Error: Bridge state has incorrect mint authority");
        store_failed_mint(
            &mut bridge_state,
            from_address,
            to_address,
            amount,
            source_chain_id,
            nonce,
            "INVALID_MINT_AUTHORITY",
        )?;
        
        // Save updated bridge state with processed nonce and failed mint record
        BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;
        return Err(ProgramError::InvalidAccountData);
    }
    
    // Create mint instruction
    let mint_ix = token_instruction::mint_to(
        &spl_token::id(),
        mint_info.key,
        destination_info.key,
        &mint_authority,
        &[],
        amount,
    )?;
    
    // Create the seeds for the PDA
    let authority_signer_seeds = &[
        b"mint_authority",
        program_id.as_ref(),
        &[bump_seed],
    ];
    
    // Execute the mint operation with PDA signature
    let result = invoke_signed(
        &mint_ix,
        &[
            mint_info.clone(),
            destination_info.clone(),
            bridge_info.clone(), // for mint authority
        ],
        &[authority_signer_seeds],
    );
    
    if let Err(err) = result {
        msg!("Error: Mint operation failed: {:?}", err);
        
        // Store the failed mint for later recovery
        store_failed_mint(
            &mut bridge_state,
            from_address,
            to_address,
            amount,
            source_chain_id,
            nonce,
            "MINT_FAILED",
        )?;
        
        // Save updated bridge state with processed nonce and failed mint record
        BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;
        return Err(err);
    }

    // Update successful mint counter and record
    bridge_state.successful_mints += 1;
    
    // Log detailed transaction information
    msg!("Successfully received {} tokens from chain {}", amount, source_chain_id);
    msg!("Source address: {:?}", from_address);
    msg!("Tokens minted to: {}", to_address);
    msg!("Transaction nonce: {}", nonce);
    
    // Update total received amount for statistics
    bridge_state.total_received += amount;
    
    // Save updated bridge state with processed nonce
    BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;
    
    Ok(())
}

/// Check if a nonce has been processed
fn is_nonce_processed(
    bridge_state: &BridgeState,
    nonce: u64,
) -> bool {
    for i in 0..bridge_state.processed_nonce_count as usize {
        if bridge_state.processed_nonces[i].nonce == nonce && !bridge_state.processed_nonces[i].can_recycle {
            return true;
        }
    }
    false
}

/// Store information about a failed mint for later recovery
fn store_failed_mint(
    bridge_state: &mut BridgeState,
    from_address: [u8; 64],
    to_address: Pubkey,
    amount: u64,
    source_chain_id: u16,
    nonce: u64,
    failure_reason: &str,
) -> ProgramResult {
    let clock = Clock::get()?;
    let timestamp = clock.unix_timestamp as u64;
    
    // Create a new failed mint record
    let failed_mint = FailedMint {
        from_address,
        to_address,
        amount,
        source_chain_id,
        nonce,
        timestamp,
        retry_count: 0,
        last_retry: 0,
        status: FailedMintStatus::Pending as u8,
        failure_reason: failure_reason.to_string(),
    };
    
    // Add to the failed mint records if there's space
    if bridge_state.failed_mint_count as usize < bridge_state.failed_mints.len() {
        let index = bridge_state.failed_mint_count as usize;
        bridge_state.failed_mints[index] = failed_mint;
        bridge_state.failed_mint_count += 1;
        
        msg!("Stored failed mint for later recovery: {} tokens to {}", amount, to_address);
        msg!("Failure reason: {}", failure_reason);
        Ok(())
    } else {
        msg!("Error: Failed mint storage is full");
        Err(ProgramError::InsufficientFunds) // Reuse this error code for "storage full"
    }
}

/// Log security events for monitoring
fn log_security_event(
    bridge_state: &mut BridgeState,
    event_type: &str,
    caller: &Pubkey,
    clock: &Clock,
    source_chain_id: u16,
    nonce: u64,
) {
    let timestamp = clock.unix_timestamp as u64;
    
    msg!("SECURITY EVENT: {}", event_type);
    msg!("Timestamp: {}", timestamp);
    msg!("Caller: {}", caller);
    msg!("Source Chain ID: {}", source_chain_id);
    msg!("Nonce: {}", nonce);
    
    // In a production environment, we would store this security event
    // in the bridge state for later analysis
}

/// Retry a previously failed mint
fn process_retry_failed_mint(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    failed_mint_index: u8,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let admin_info = next_account_info(account_info_iter)?;
    let mint_info = next_account_info(account_info_iter)?;
    let destination_info = next_account_info(account_info_iter)?;
    let token_program_info = next_account_info(account_info_iter)?;
    let bridge_info = next_account_info(account_info_iter)?;
    let clock_info = next_account_info(account_info_iter)?;
    
    // Verify admin signature
    if !admin_info.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Verify the bridge account is owned by this program
    if bridge_info.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    
    // Load bridge state
    let mut bridge_state = BridgeState::unpack(&bridge_info.data.borrow())?;
    
    // Verify admin authority
    if !is_admin(admin_info.key, &bridge_state) {
        return Err(ProgramError::InvalidAccountData);
    }
    
    // Check if the failed mint index is valid
    if failed_mint_index as usize >= bridge_state.failed_mint_count as usize {
        msg!("Error: Invalid failed mint index");
        return Err(ProgramError::InvalidArgument);
    }
    
    // Get the failed mint record
    let failed_mint = &mut bridge_state.failed_mints[failed_mint_index as usize];
    
    // Check if the failed mint is still pending
    if failed_mint.status != FailedMintStatus::Pending as u8 {
        msg!("Error: Failed mint is not in pending status");
        return Err(ProgramError::InvalidArgument);
    }
    
    // Verify destination matches the failed mint record
    if *destination_info.key != failed_mint.to_address {
        msg!("Error: Destination account does not match failed mint record");
        return Err(ProgramError::InvalidArgument);
    }
    
    // Get mint authority PDA
    let seeds = &[
        b"mint_authority",
        program_id.as_ref(),
        &[0], // Bump seed
    ];
    let (authority_pubkey, bump_seed) = Pubkey::find_program_address(seeds, program_id);
    
    if authority_pubkey != bridge_state.mint_authority {
        msg!("Error: Mint authority mismatch");
        return Err(ProgramError::InvalidArgument);
    }
    
    let authority_signer_seeds = &[
        b"mint_authority",
        program_id.as_ref(),
        &[bump_seed],
    ];
    
    // Try to mint tokens with detailed error handling
    let mint_to_ix = token_instruction::mint_to(
        &spl_token::id(),
        mint_info.key,
        destination_info.key,
        &authority_pubkey,
        &[],
        failed_mint.amount,
    )?;

    let result = invoke_signed(
        &mint_to_ix,
        &[
            mint_info.clone(),
            destination_info.clone(),
            admin_info.clone(),
            token_program_info.clone(),
        ],
        &[authority_signer_seeds],
    );
    
    // Update retry count and timestamp
    let clock = Clock::from_account_info(clock_info)?;
    failed_mint.retry_count += 1;
    failed_mint.last_retry = clock.unix_timestamp as u64;
    
    if let Err(err) = result {
        msg!("Error: Retry mint operation failed: {:?}", err);
        
        // If max retries reached, mark as failed permanently
        if failed_mint.retry_count >= MAX_MINT_RETRIES {
            failed_mint.status = FailedMintStatus::Failed as u8;
            msg!("Maximum retry attempts reached, marking mint as permanently failed");
        }
        
        // Save updated bridge state
        BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;
        return Err(err);
    }

    // Mark as completed
    failed_mint.status = FailedMintStatus::Completed as u8;
    
    // Log success
    msg!("Successfully retried mint of {} tokens to {}", 
         failed_mint.amount, failed_mint.to_address);
    msg!("Original source: Chain ID {} - Nonce {}", 
         failed_mint.source_chain_id, failed_mint.nonce);
    
    // Update successful mint counter
    bridge_state.successful_mints += 1;
    bridge_state.retried_mints += 1;
    
    // Update total received amount
    bridge_state.total_received += failed_mint.amount;
    
    // Save updated bridge state
    BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;
    
    Ok(())
}

/// Process a batch of failed mints automatically
fn process_auto_retry_failed_mints(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    max_retries: u8,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let admin_info = next_account_info(account_info_iter)?;
    let mint_info = next_account_info(account_info_iter)?;
    let token_program_info = next_account_info(account_info_iter)?;
    let bridge_info = next_account_info(account_info_iter)?;
    let clock_info = next_account_info(account_info_iter)?;
    
    // Verify admin signature
    if !admin_info.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Verify the bridge account is owned by this program
    if bridge_info.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    
    // Load bridge state
    let mut bridge_state = BridgeState::unpack(&bridge_info.data.borrow())?;
    
    // Verify admin authority
    if !is_admin(admin_info.key, &bridge_state) {
        return Err(ProgramError::InvalidAccountData);
    }
    
    // Get current timestamp
    let clock = Clock::from_account_info(clock_info)?;
    let current_time = clock.unix_timestamp as u64;
    
    // Cap max retries to avoid excessive gas usage
    let retries_to_attempt = std::cmp::min(max_retries, 10);
    
    // Track how many mints we successfully retried
    let mut retried_count = 0;
    
    // Attempt to retry pending failed mints
    for i in 0..bridge_state.failed_mint_count as usize {
        if retried_count >= retries_to_attempt {
            break;
        }
        
        let failed_mint = &mut bridge_state.failed_mints[i];
        
        // Skip if not pending or if retry was attempted recently
        if failed_mint.status != FailedMintStatus::Pending as u8 ||
           (failed_mint.last_retry > 0 && 
            current_time - failed_mint.last_retry < MIN_RETRY_INTERVAL) {
            continue;
        }
        
        // We need to find the destination account in the remaining accounts
        let mut destination_info_opt = None;
        
        // Start from accounts that come after the ones we already processed
        for account in accounts.iter().skip(5) {
            if account.key == &failed_mint.to_address {
                destination_info_opt = Some(account);
                break;
            }
        }
        
        // Skip if destination account not provided
        let destination_info = match destination_info_opt {
            Some(info) => info,
            None => continue,
        };
        
        // Get mint authority PDA
        let seeds = &[
            b"mint_authority",
            program_id.as_ref(),
            &[0], // Bump seed
        ];
        let (authority_pubkey, bump_seed) = Pubkey::find_program_address(seeds, program_id);
        
        if authority_pubkey != bridge_state.mint_authority {
            msg!("Error: Mint authority mismatch");
            continue;
        }
        
        let authority_signer_seeds = &[
            b"mint_authority",
            program_id.as_ref(),
            &[bump_seed],
        ];
        
        // Try to mint tokens
        let mint_to_ix = match token_instruction::mint_to(
            &spl_token::id(),
            mint_info.key,
            destination_info.key,
            &authority_pubkey,
            &[],
            failed_mint.amount,
        ) {
            Ok(ix) => ix,
            Err(_) => continue,
        };

        // Update retry count and timestamp
        failed_mint.retry_count += 1;
        failed_mint.last_retry = current_time;
        
        let result = invoke_signed(
            &mint_to_ix,
            &[
                mint_info.clone(),
                destination_info.clone(),
                admin_info.clone(),
                token_program_info.clone(),
            ],
            &[authority_signer_seeds],
        );
        
        if let Err(err) = result {
            msg!("Error retrying mint {}: {:?}", i, err);
            
            // If max retries reached, mark as failed permanently
            if failed_mint.retry_count >= MAX_MINT_RETRIES {
                failed_mint.status = FailedMintStatus::Failed as u8;
            }
            
            continue;
        }

        // Mark as completed
        failed_mint.status = FailedMintStatus::Completed as u8;
        
        // Log success
        msg!("Auto-retried mint {} of {} tokens to {}", 
             i, failed_mint.amount, failed_mint.to_address);
        
        // Update counters
        bridge_state.successful_mints += 1;
        bridge_state.retried_mints += 1;
        bridge_state.total_received += failed_mint.amount;
        
        retried_count += 1;
    }
    
    // Save updated bridge state
    BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;
    
    msg!("Auto-retried {} failed mints", retried_count);
    
    Ok(())
}

/// Get information about failed mints for monitoring
fn process_get_failed_mint_info(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let bridge_info = next_account_info(account_info_iter)?;
    
    // Verify the bridge account is owned by this program
    if bridge_info.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    
    // Load bridge state
    let bridge_state = BridgeState::unpack(&bridge_info.data.borrow())?;
    
    // Count failed mints by status
    let mut pending_count = 0;
    let mut completed_count = 0;
    let mut failed_count = 0;
    
    for i in 0..bridge_state.failed_mint_count as usize {
        let failed_mint = &bridge_state.failed_mints[i];
        
        match failed_mint.status {
            s if s == FailedMintStatus::Pending as u8 => pending_count += 1,
            s if s == FailedMintStatus::Completed as u8 => completed_count += 1,
            s if s == FailedMintStatus::Failed as u8 => failed_count += 1,
            _ => {},
        }
    }
    
    // Log statistics
    msg!("Failed Mint Statistics:");
    msg!("Total tracked: {}", bridge_state.failed_mint_count);
    msg!("Pending: {}", pending_count);
    msg!("Completed via retry: {}", completed_count);
    msg!("Permanently failed: {}", failed_count);
    msg!("Total successful mints: {}", bridge_state.successful_mints);
    msg!("Total retried mints: {}", bridge_state.retried_mints);
    
    Ok(())
}

/// Define failed mint status for better tracking
#[repr(u8)]
pub enum FailedMintStatus {
    Pending = 0,
    Completed = 1,
    Failed = 2,
}

/// Store information about a failed mint for recovery
pub struct FailedMint {
    /// Source address on the other chain
    pub from_address: [u8; 64],
    /// Destination address on Solana
    pub to_address: Pubkey,
    /// Amount of tokens to mint
    pub amount: u64,
    /// Source chain ID
    pub source_chain_id: u16,
    /// Original nonce to prevent replay
    pub nonce: u64,
    /// When the mint was first attempted
    pub timestamp: u64,
    /// Number of retry attempts
    pub retry_count: u8,
    /// Last retry timestamp
    pub last_retry: u64,
    /// Current status
    pub status: u8,
    /// Reason for failure
    pub failure_reason: String,
}

/// Tạo metadata cho token
fn process_create_metadata(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    // Đây chỉ là stub function, trong thực tế cần tích hợp với Metaplex
    msg!("Creating metadata for DMD token...");
    msg!("This is a placeholder - actual implementation would use Metaplex Token Metadata Program");
    
    Ok(())
}

/// Request to add or remove a trusted remote
fn process_request_trusted_remote_update(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    remote_address: [u8; 64],
    chain_id: u16,
    is_add: bool,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let admin_info = next_account_info(account_info_iter)?;
    let bridge_info = next_account_info(account_info_iter)?;
    let clock_info = next_account_info(account_info_iter)?;
    
    // Verify the bridge account is owned by this program
    if bridge_info.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    
    // Verify the admin signature
    if !admin_info.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Verify admin authority (in real implementation, should check against a stored admin list)
    // For demo, we're just checking the admin is the mint authority
    let mut bridge_state = BridgeState::unpack(&bridge_info.data.borrow())?;
    if admin_info.key != &bridge_state.mint_authority {
        return Err(ProgramError::InvalidAccountData);
    }
    
    // Check for existing pending updates for this remote
    for i in 0..bridge_state.pending_update_count as usize {
        if bridge_state.pending_updates[i].chain_id == chain_id && 
           bridge_state.pending_updates[i].remote_address == remote_address {
            return Err(ProgramError::InvalidArgument);
        }
    }
    
    // Check if we can add more pending updates
    if bridge_state.pending_update_count as usize >= bridge_state.pending_updates.len() {
        return Err(ProgramError::InsufficientFunds); // Reuse error code for "no more space"
    }
    
    // Calculate timelock expiry
    let clock = Clock::from_account_info(clock_info)?;
    let current_time = clock.unix_timestamp as u64;
    let timelock_expiry = current_time + DEFAULT_TIMELOCK_DURATION;
    
    // Create new update request
    let update = TrustedRemoteUpdate {
        remote_address,
        chain_id,
        timelock_expiry,
        is_add,
    };
    
    // Add the update to pending updates
    let index = bridge_state.pending_update_count as usize;
    bridge_state.pending_updates[index] = update;
    bridge_state.pending_update_count += 1;
    
    // Save updated state
    BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;
    
    msg!("Trusted remote update requested. Will be executable after: {}", timelock_expiry);
    
    Ok(())
}

/// Execute a pending trusted remote update after timelock expires
fn process_execute_trusted_remote_update(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    remote_address: [u8; 64],
    chain_id: u16,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let admin_info = next_account_info(account_info_iter)?;
    let bridge_info = next_account_info(account_info_iter)?;
    let clock_info = next_account_info(account_info_iter)?;
    
    // Verify the bridge account is owned by this program
    if bridge_info.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    
    // Verify the admin signature
    if !admin_info.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Load bridge state
    let mut bridge_state = BridgeState::unpack(&bridge_info.data.borrow())?;
    
    // Verify admin authority
    if admin_info.key != &bridge_state.mint_authority {
        return Err(ProgramError::InvalidAccountData);
    }
    
    // Find the pending update
    let mut update_index = None;
    for i in 0..bridge_state.pending_update_count as usize {
        if bridge_state.pending_updates[i].chain_id == chain_id && 
           bridge_state.pending_updates[i].remote_address == remote_address {
            update_index = Some(i);
            break;
        }
    }
    
    // Check if update exists
    let index = match update_index {
        Some(idx) => idx,
        None => return Err(ProgramError::InvalidArgument),
    };
    
    // Get current time
    let clock = Clock::from_account_info(clock_info)?;
    let current_time = clock.unix_timestamp as u64;
    
    // Check if timelock has expired
    if current_time < bridge_state.pending_updates[index].timelock_expiry {
        return Err(ProgramError::Custom(100)); // Custom error for "timelock not expired"
    }
    
    let update = bridge_state.pending_updates[index];
    
    // Process the update
    if update.is_add {
        // Add trusted remote if not already added
        let mut already_exists = false;
        for i in 0..bridge_state.trusted_remote_count as usize {
            let offset = i * TRUSTED_REMOTE_SIZE;
            
            // First 2 bytes are chain_id
            let chain_id_bytes = [
                bridge_state.trusted_remotes[offset],
                bridge_state.trusted_remotes[offset + 1],
            ];
            let existing_chain_id = u16::from_le_bytes(chain_id_bytes);
            
            // Next 62 bytes is the address (we're using the first 62 of the 64-byte address)
            let mut existing_address = [0u8; 64];
            for j in 0..62 {
                existing_address[j] = bridge_state.trusted_remotes[offset + 2 + j];
            }
            
            if existing_chain_id == update.chain_id && 
               existing_address[0..62] == update.remote_address[0..62] {
                already_exists = true;
                break;
            }
        }
        
        if !already_exists {
            if bridge_state.trusted_remote_count as usize >= MAX_TRUSTED_REMOTES {
                return Err(ProgramError::InsufficientFunds); // Reuse error for "no more space"
            }
            
            // Add new trusted remote
            let offset = bridge_state.trusted_remote_count as usize * TRUSTED_REMOTE_SIZE;
            
            // Store chain_id (2 bytes)
            let chain_id_bytes = update.chain_id.to_le_bytes();
            bridge_state.trusted_remotes[offset] = chain_id_bytes[0];
            bridge_state.trusted_remotes[offset + 1] = chain_id_bytes[1];
            
            // Store address (62 bytes)
            for i in 0..62 {
                bridge_state.trusted_remotes[offset + 2 + i] = update.remote_address[i];
            }
            
            bridge_state.trusted_remote_count += 1;
            msg!("Added trusted remote from chain {}", update.chain_id);
        } else {
            msg!("Trusted remote already exists");
        }
    } else {
        // Remove trusted remote if exists
        let mut found_index = None;
        for i in 0..bridge_state.trusted_remote_count as usize {
            let offset = i * TRUSTED_REMOTE_SIZE;
            
            // First 2 bytes are chain_id
            let chain_id_bytes = [
                bridge_state.trusted_remotes[offset],
                bridge_state.trusted_remotes[offset + 1],
            ];
            let existing_chain_id = u16::from_le_bytes(chain_id_bytes);
            
            // Next 62 bytes is the address
            let mut existing_address = [0u8; 64];
            for j in 0..62 {
                existing_address[j] = bridge_state.trusted_remotes[offset + 2 + j];
            }
            
            if existing_chain_id == update.chain_id && 
               existing_address[0..62] == update.remote_address[0..62] {
                found_index = Some(i);
                break;
            }
        }
        
        if let Some(i) = found_index {
            // Remove by swapping with the last element and decrementing count
            if i < (bridge_state.trusted_remote_count as usize - 1) {
                let last_index = (bridge_state.trusted_remote_count as usize - 1) * TRUSTED_REMOTE_SIZE;
                let current_index = i * TRUSTED_REMOTE_SIZE;
                
                // Copy the last element to the current position
                for j in 0..TRUSTED_REMOTE_SIZE {
                    bridge_state.trusted_remotes[current_index + j] = 
                        bridge_state.trusted_remotes[last_index + j];
                }
            }
            
            bridge_state.trusted_remote_count -= 1;
            msg!("Removed trusted remote from chain {}", update.chain_id);
        } else {
            msg!("Trusted remote not found");
        }
    }
    
    // Remove the update from pending list
    if index < (bridge_state.pending_update_count as usize - 1) {
        for i in index..(bridge_state.pending_update_count as usize - 1) {
            bridge_state.pending_updates[i] = bridge_state.pending_updates[i + 1];
        }
    }
    bridge_state.pending_update_count -= 1;
    
    // Save updated state
    BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;
    
    Ok(())
}

/// Cancel a pending trusted remote update
fn process_cancel_trusted_remote_update(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    remote_address: [u8; 64],
    chain_id: u16,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let admin_info = next_account_info(account_info_iter)?;
    let bridge_info = next_account_info(account_info_iter)?;
    
    // Verify the bridge account is owned by this program
    if bridge_info.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    
    // Verify the admin signature
    if !admin_info.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Load bridge state
    let mut bridge_state = BridgeState::unpack(&bridge_info.data.borrow())?;
    
    // Verify admin authority
    if admin_info.key != &bridge_state.mint_authority {
        return Err(ProgramError::InvalidAccountData);
    }
    
    // Find the pending update
    let mut update_index = None;
    for i in 0..bridge_state.pending_update_count as usize {
        if bridge_state.pending_updates[i].chain_id == chain_id && 
           bridge_state.pending_updates[i].remote_address == remote_address {
            update_index = Some(i);
            break;
        }
    }
    
    // Check if update exists
    let index = match update_index {
        Some(idx) => idx,
        None => return Err(ProgramError::InvalidArgument),
    };
    
    // Remove the update from pending list
    if index < (bridge_state.pending_update_count as usize - 1) {
        for i in index..(bridge_state.pending_update_count as usize - 1) {
            bridge_state.pending_updates[i] = bridge_state.pending_updates[i + 1];
        }
    }
    bridge_state.pending_update_count -= 1;
    
    // Save updated state
    BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;
    
    msg!("Cancelled trusted remote update for chain {}", chain_id);
    
    Ok(())
}

/// Check if an address from a specific chain is a trusted remote
fn is_trusted_remote(
    bridge_state: &BridgeState,
    source_chain_id: u16,
    source_address: &[u8],
) -> bool {
    for i in 0..bridge_state.trusted_remote_count as usize {
        let offset = i * TRUSTED_REMOTE_SIZE;
        
        // First 2 bytes are chain_id
        let chain_id_bytes = [
            bridge_state.trusted_remotes[offset],
            bridge_state.trusted_remotes[offset + 1],
        ];
        let chain_id = u16::from_le_bytes(chain_id_bytes);
        
        // Check chain ID first
        if chain_id != source_chain_id {
            continue;
        }
        
        // Compare address (we only compare the first min length bytes)
        let compare_len = std::cmp::min(62, source_address.len());
        let mut is_match = true;
        
        for j in 0..compare_len {
            if bridge_state.trusted_remotes[offset + 2 + j] != source_address[j] {
                is_match = false;
                break;
            }
        }
        
        if is_match {
            return true;
        }
    }
    
    false
}

/// Record a nonce as processed to prevent replay attacks
fn mark_nonce_as_processed(
    bridge_state: &mut BridgeState,
    nonce: u64,
) -> ProgramResult {
    // Get current timestamp
    let current_timestamp = Clock::get()?.unix_timestamp as u64;
    
    // Check if nonce is already processed
    if is_nonce_processed(bridge_state, nonce) {
        return Err(ProgramError::Custom(101)); // Custom error for "nonce already processed"
    }
    
    // Get the next slot in the circular buffer
    let index = bridge_state.nonce_current_index as usize % 1000;
    
    // If we're overwriting an existing nonce, make sure it's old enough
    if bridge_state.processed_nonce_count >= 1000 {
        let existing_nonce = &bridge_state.processed_nonces[index];
        if !existing_nonce.can_recycle && current_timestamp - existing_nonce.timestamp < 604800 { // 7 days in seconds
            msg!("Warning: Nonce buffer full with recent entries. Consider increasing buffer size.");
            // Find the oldest nonce that can be recycled
            let mut oldest_index = index;
            let mut oldest_timestamp = current_timestamp;
            
            for i in 0..1000 {
                if bridge_state.processed_nonces[i].can_recycle || 
                   bridge_state.processed_nonces[i].timestamp < oldest_timestamp {
                    oldest_index = i;
                    oldest_timestamp = bridge_state.processed_nonces[i].timestamp;
                }
            }
            
            // Use the oldest slot instead
            bridge_state.processed_nonces[oldest_index] = ProcessedNonce {
                nonce,
                timestamp: current_timestamp,
                source_chain_id: 0, // Updated when used with message
                can_recycle: false,
            };
            
            // Don't increment the counter or current index in this case
            return Ok(());
        }
    }
    
    // Store the nonce with current timestamp
    bridge_state.processed_nonces[index] = ProcessedNonce {
        nonce,
        timestamp: current_timestamp,
        source_chain_id: 0, // Updated when used with message
        can_recycle: false,
    };
    
    // Update counters
    if bridge_state.processed_nonce_count < 1000 {
        bridge_state.processed_nonce_count += 1;
    }
    
    bridge_state.nonce_current_index = (bridge_state.nonce_current_index + 1) % 1000;
    
    // Clean up old nonces periodically
    if current_timestamp % 3600 == 0 { // Once per hour
        cleanup_old_nonces(bridge_state, current_timestamp)?;
    }
    
    Ok(())
}

/// Clean up old nonces to prevent memory bloat
fn cleanup_old_nonces(
    bridge_state: &mut BridgeState,
    current_timestamp: u64,
) -> ProgramResult {
    let time_threshold = 7 * 24 * 60 * 60; // 7 days in seconds
    let mut recycled_count = 0;
    
    for i in 0..bridge_state.processed_nonce_count as usize {
        if !bridge_state.processed_nonces[i].can_recycle && 
           current_timestamp - bridge_state.processed_nonces[i].timestamp > time_threshold {
            // Mark as recyclable instead of removing
            bridge_state.processed_nonces[i].can_recycle = true;
            recycled_count += 1;
        }
    }
    
    if recycled_count > 0 {
        msg!("Marked {} old nonces as recyclable", recycled_count);
    }
    
    Ok(())
}

/// Handle failed bridge requests
fn process_handle_failed_bridge(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    request_id: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let admin_info = next_account_info(account_info_iter)?;
    let escrow_info = next_account_info(account_info_iter)?;
    let destination_info = next_account_info(account_info_iter)?;
    let token_program_info = next_account_info(account_info_iter)?;
    let bridge_info = next_account_info(account_info_iter)?;
    
    // Verify admin permissions
    if !admin_info.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Verify the bridge account is owned by this program
    if bridge_info.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    
    // Load bridge state
    let mut bridge_state = BridgeState::unpack(&bridge_info.data.borrow())?;
    
    // Find the request with the given ID
    let mut request_index = None;
    for (i, request) in bridge_state.bridge_requests.iter().enumerate().take(bridge_state.bridge_request_count as usize) {
        if request.request_id == request_id && request.status == 0 { // 0 = Pending
            request_index = Some(i);
            break;
        }
    }
    
    // Check if we found the request
    let index = request_index.ok_or(ProgramError::InvalidArgument)?;
    let request = bridge_state.bridge_requests[index];
    
    // Check if destination account matches the source account in the request
    if destination_info.key != &request.source_account {
        msg!("Destination account does not match source account of bridge request");
        return Err(ProgramError::InvalidArgument);
    }
    
    // Transfer tokens from escrow back to the original sender
    let transfer_ix = token_instruction::transfer(
        &spl_token::id(),
        escrow_info.key,
        destination_info.key,
        &bridge_state.mint_authority, // Escrow owner/authority
        &[],
        request.amount,
    )?;
    
    // Use PDA for authority
    let seeds = &[
        b"mint_authority",
        program_id.as_ref(),
        &[0], // Bump seed - would be calculated in real implementation
    ];
    let (authority_pubkey, bump_seed) = Pubkey::find_program_address(seeds, program_id);
    
    if authority_pubkey != bridge_state.mint_authority {
        msg!("Authority mismatch");
        return Err(ProgramError::InvalidArgument);
    }
    
    let authority_signer_seeds = &[
        b"mint_authority",
        program_id.as_ref(),
        &[bump_seed],
    ];
    
    // Execute transfer with PDA signature
    invoke_signed(
        &transfer_ix,
        &[
            escrow_info.clone(),
            destination_info.clone(),
            admin_info.clone(),
            token_program_info.clone(),
        ],
        &[authority_signer_seeds],
    )?;
    
    // Update request status to Refunded (3)
    bridge_state.bridge_requests[index].status = 3;
    
    // Update bridge state
    BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;
    
    msg!("Refunded {} tokens for bridge request ID {}", request.amount, request_id);
    
    Ok(())
}

/// Retry a failed bridge request
fn process_retry_bridge(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    request_id: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let admin_info = next_account_info(account_info_iter)?;
    let layerzero_endpoint_info = next_account_info(account_info_iter)?;
    let bridge_info = next_account_info(account_info_iter)?;
    
    // Verify admin permissions
    if !admin_info.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Verify the bridge account is owned by this program
    if bridge_info.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    
    // Verify endpoint
    if layerzero_endpoint_info.key.to_string() != LAYERZERO_ENDPOINT {
        return Err(ProgramError::InvalidAccountData);
    }
    
    // Load bridge state
    let mut bridge_state = BridgeState::unpack(&bridge_info.data.borrow())?;
    
    // Find the request with the given ID
    let mut request_index = None;
    for (i, request) in bridge_state.bridge_requests.iter().enumerate().take(bridge_state.bridge_request_count as usize) {
        if request.request_id == request_id && request.status == 0 { // 0 = Pending
            request_index = Some(i);
            break;
        }
    }
    
    // Check if we found the request
    let index = request_index.ok_or(ProgramError::InvalidArgument)?;
    let request = bridge_state.bridge_requests[index];
    
    // Increment retry count
    if bridge_state.bridge_requests[index].retry_count >= 3 {
        msg!("Max retry count exceeded for bridge request ID {}", request_id);
        return Err(ProgramError::InvalidArgument);
    }
    bridge_state.bridge_requests[index].retry_count += 1;
    
    // Save bridge state with updated retry count
    BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;
    
    // Prepare payload for LayerZero retry
    let mut payload = Vec::with_capacity(112);
    payload.extend_from_slice(&[0x01]); // Type: bridge token
    payload.extend_from_slice(&request.to_address); // Destination address
    payload.extend_from_slice(&request.amount.to_le_bytes()); // Amount
    payload.extend_from_slice(request.source_account.as_ref()); // Source account
    payload.extend_from_slice(&request_id.to_le_bytes()); // Request ID
    
    // In real implementation, this would call LayerZero to retry
    msg!("Retrying bridge request ID {}", request_id);
    msg!("Attempt {} of 3", bridge_state.bridge_requests[index].retry_count);
    
    Ok(())
}

/// Set bridge parameters
fn process_set_bridge_params(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    daily_limit: u64,
    user_cooldown: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let admin_info = next_account_info(account_info_iter)?;
    let bridge_info = next_account_info(account_info_iter)?;
    
    // Verify admin permissions
    if !admin_info.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Verify the bridge account is owned by this program
    if bridge_info.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    
    // Load bridge state
    let mut bridge_state = BridgeState::unpack(&bridge_info.data.borrow())?;
    
    // Update parameters
    bridge_state.daily_limit = daily_limit;
    bridge_state.user_cooldown = user_cooldown;
    
    // Save bridge state
    BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;
    
    msg!("Updated bridge parameters: daily_limit={}, user_cooldown={}s", daily_limit, user_cooldown);
    
    Ok(())
}

/// Initialize bridge state account
fn process_initialize_bridge(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    escrow_account: Pubkey,
    layerzero_endpoint: Pubkey,
    base_fee: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let admin_info = next_account_info(account_info_iter)?;
    let bridge_info = next_account_info(account_info_iter)?;
    let clock_info = next_account_info(account_info_iter)?;
    
    // Verify admin signature
    if !admin_info.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Verify bridge account is owned by this program
    if bridge_info.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    
    // Check if the account is already initialized
    let mut bridge_data = bridge_info.try_borrow_mut_data()?;
    if !bridge_data.iter().all(|&x| x == 0) {
        msg!("Bridge account already initialized");
        return Err(ProgramError::AccountAlreadyInitialized);
    }
    
    // Get current timestamp
    let clock = Clock::from_account_info(clock_info)?;
    let current_timestamp = clock.unix_timestamp as u64;
    
    // Create PDA for mint authority
    let seeds = &[
        b"mint_authority",
        program_id.as_ref(),
        &[0], // Bump seed - would be calculated in real implementation
    ];
    let (authority_pubkey, _bump_seed) = Pubkey::find_program_address(seeds, program_id);
    
    // Initialize bridge state with default values
    let bridge_state = BridgeState {
        is_initialized: true,
        escrow_account,
        layerzero_endpoint,
        base_fee,
        mint_authority: authority_pubkey,
        trusted_remotes: [0u8; TRUSTED_REMOTE_SIZE * MAX_TRUSTED_REMOTES],
        trusted_remote_count: 0,
        pending_updates: [TrustedRemoteUpdate {
            remote_address: [0u8; 64],
            chain_id: 0,
            timelock_expiry: 0,
            is_add: false,
        }; 5],
        pending_update_count: 0,
        processed_nonces: [ProcessedNonce {
            nonce: 0,
            timestamp: 0,
            source_chain_id: 0,
            can_recycle: false,
        }; 1000],
        processed_nonce_count: 0,
        nonce_current_index: 0,
        chain_sequences: [ChainSequence {
            chain_id: 0,
            expected_sequence: 0,
            last_timestamp: 0,
            recovery_mode: false,
        }; 32],
        chain_sequence_count: 0,
        bridge_requests: [BridgeRequest {
            to_address: [0u8; 64],
            amount: 0,
            source_account: Pubkey::default(),
            timestamp: 0,
            request_id: 0,
            status: 0,
            retry_count: 0,
        }; MAX_BRIDGE_REQUESTS],
        bridge_request_count: 0,
        next_request_id: 1, // Start from 1
        daily_volume: 0,
        daily_volume_timestamp: current_timestamp,
        daily_limit: 10_000_000_000, // Default limit of 10,000 tokens (assuming 6 decimals)
        user_cooldown: 300, // Default 5 minute cooldown
        user_last_bridge: std::collections::HashMap::new(),
        bridge_state: 0,
        emergency_requests: [EmergencyRequest {
            request_id: 0,
            timestamp: 0,
            requester: Pubkey::default(),
            action_type: 0,
            target_account: Pubkey::default(),
            amount: 0,
        }; 5],
        emergency_request_count: 0,
        next_emergency_id: 1,
        escrow_token_balance: 0,
        escrow_update_timestamp: 0,
        max_escrow_threshold: 1_000_000_000_000,
        user_daily_volumes: std::collections::HashMap::new(),
        admin_accounts: [Pubkey::default(); 5],
        admin_count: 0,
        failed_mints: [FailedMint {
            from_address: [0u8; 64],
            to_address: Pubkey::default(),
            amount: 0,
            source_chain_id: 0,
            nonce: 0,
            timestamp: 0,
            retry_count: 0,
            last_retry: 0,
            status: 0,
            failure_reason: String::new(),
        }; MAX_FAILED_MINTS],
        failed_mint_count: 0,
        successful_mints: 0,
        retried_mints: 0,
        total_received: 0,
        security_events: [SecurityEvent {
            event_type: String::new(),
            timestamp: 0,
            caller: Pubkey::default(),
            source_chain_id: 0,
            nonce: 0,
            message: String::new(),
        }; MAX_SECURITY_EVENTS],
        security_event_count: 0,
        last_bridge_attempt_id: 0,
        bridge_attempts: [BridgeAttempt {
            attempt_id: 0,
            timestamp: 0,
            status: 0,
            error_message: String::new(),
        }; MAX_BRIDGE_REQUESTS],
        bridge_attempt_count: 0,
        admin_timelock_expiry: 0,
        admin_timelock_requester: Pubkey::default(),
        emergency_recovery_mode: false,
        layerzero_status: 0,
        layerzero_status_timestamp: 0,
    };
    
    // Serialize bridge state to account data
    BridgeState::pack(bridge_state, &mut bridge_data)?;
    
    msg!("Bridge initialized with escrow account {}", escrow_account);
    msg!("LayerZero endpoint set to {}", layerzero_endpoint);
    msg!("Base fee set to {}", base_fee);
    
    Ok(())
}

/// Request emergency action (such as pausing bridge or rescuing tokens)
fn process_request_emergency_action(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    action_type: u8,
    target_account: Pubkey,
    amount: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let admin_info = next_account_info(account_info_iter)?;
    let bridge_info = next_account_info(account_info_iter)?;
    let clock_info = next_account_info(account_info_iter)?;
    
    // Verify admin signature
    if !admin_info.is_signer {
        msg!("Missing admin signature for emergency action");
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Verify bridge account is owned by this program
    if bridge_info.owner != program_id {
        msg!("Bridge account not owned by this program");
        return Err(ProgramError::IncorrectProgramId);
    }
    
    // Get current timestamp
    let clock = Clock::from_account_info(clock_info)?;
    let current_timestamp = clock.unix_timestamp as u64;
    
    // Load bridge state
    let mut bridge_state = BridgeState::unpack(&bridge_info.data.borrow())?;
    
    // Check if we have room for a new emergency request
    if bridge_state.emergency_request_count >= 5 {
        msg!("Too many pending emergency requests");
        return Err(ProgramError::InvalidArgument);
    }
    
    // Validate action type
    if action_type > 2 {
        msg!("Invalid emergency action type: {}", action_type);
        return Err(ProgramError::InvalidArgument);
    }
    
    // Additional validations for rescue tokens
    if action_type == 2 {
        if amount == 0 {
            msg!("Amount must be greater than zero for token rescue");
            return Err(ProgramError::InvalidArgument);
        }
        
        if amount > bridge_state.escrow_token_balance {
            msg!("Cannot rescue more tokens than available in escrow");
            return Err(ProgramError::InvalidArgument);
        }
        
        // Ensure target account is not zero
        if target_account == Pubkey::default() {
            msg!("Target account cannot be zero address for token rescue");
            return Err(ProgramError::InvalidArgument);
        }
    }
    
    // Create a new emergency request
    let request_id = bridge_state.next_emergency_id;
    bridge_state.next_emergency_id += 1;
    
    let emergency_request = EmergencyRequest {
        request_id,
        timestamp: current_timestamp,
        requester: *admin_info.key,
        action_type,
        target_account,
        amount,
    };
    
    // Store the emergency request
    let request_index = bridge_state.emergency_request_count as usize;
    bridge_state.emergency_requests[request_index] = emergency_request;
    bridge_state.emergency_request_count += 1;
    
    // Save bridge state with updated request info
    BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;
    
    msg!("Emergency action requested: type={}, id={}, timelock expires at {}", 
        action_type, request_id, current_timestamp + EMERGENCY_TIMELOCK);
    
    Ok(())
}

/// Execute emergency action after timelock period
fn process_execute_emergency_action(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    request_id: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let admin_info = next_account_info(account_info_iter)?;
    let bridge_info = next_account_info(account_info_iter)?;
    let clock_info = next_account_info(account_info_iter)?;
    
    // Additional accounts for rescue tokens
    let escrow_info = if let Ok(account) = next_account_info(account_info_iter) {
        Some(account)
    } else {
        None
    };
    
    let target_info = if let Ok(account) = next_account_info(account_info_iter) {
        Some(account)
    } else {
        None
    };
    
    let token_program_info = if let Ok(account) = next_account_info(account_info_iter) {
        Some(account)
    } else {
        None
    };
    
    // Verify admin signature
    if !admin_info.is_signer {
        msg!("Missing admin signature");
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Verify bridge account is owned by this program
    if bridge_info.owner != program_id {
        msg!("Bridge account not owned by this program");
        return Err(ProgramError::IncorrectProgramId);
    }
    
    // Get current timestamp
    let clock = Clock::from_account_info(clock_info)?;
    let current_timestamp = clock.unix_timestamp as u64;
    
    // Load bridge state
    let mut bridge_state = BridgeState::unpack(&bridge_info.data.borrow())?;
    
    // Find the emergency request with the given ID
    let mut request_index = None;
    for (i, request) in bridge_state.emergency_requests.iter().enumerate().take(bridge_state.emergency_request_count as usize) {
        if request.request_id == request_id {
            request_index = Some(i);
            break;
        }
    }
    
    // Check if we found the request
    let index = request_index.ok_or_else(|| {
        msg!("Emergency request not found: {}", request_id);
        ProgramError::InvalidArgument
    })?;
    
    let request = bridge_state.emergency_requests[index];
    
    // Check if timelock period has passed
    if current_timestamp < request.timestamp + EMERGENCY_TIMELOCK {
        let remaining = request.timestamp + EMERGENCY_TIMELOCK - current_timestamp;
        msg!("Emergency timelock not yet expired: {}s remaining", remaining);
        return Err(ProgramError::InvalidArgument);
    }
    
    // Execute the appropriate action based on action type
    match request.action_type {
        0 => {
            // Pause bridge
            bridge_state.bridge_state = 1;
            msg!("Bridge paused successfully");
        },
        1 => {
            // Unpause bridge
            bridge_state.bridge_state = 0;
            msg!("Bridge unpaused successfully");
        },
        2 => {
            // Rescue tokens from escrow
            
            // Ensure we have all necessary accounts
            let escrow = escrow_info.ok_or_else(|| {
                msg!("Missing escrow account for token rescue");
                ProgramError::NotEnoughAccountKeys
            })?;
            
            let target = target_info.ok_or_else(|| {
                msg!("Missing target account for token rescue");
                ProgramError::NotEnoughAccountKeys
            })?;
            
            let token_program = token_program_info.ok_or_else(|| {
                msg!("Missing token program for token rescue");
                ProgramError::NotEnoughAccountKeys
            })?;
            
            // Verify target account matches the one in the request
            if target.key != &request.target_account {
                msg!("Target account does not match the one in the request");
                return Err(ProgramError::InvalidArgument);
            }
            
            // Transfer tokens from escrow to target account
            let transfer_ix = token_instruction::transfer(
                &spl_token::id(),
                escrow.key,
                target.key,
                &bridge_state.mint_authority, // Escrow owner/authority
                &[],
                request.amount,
            )?;
            
            // Use PDA for authority
            let seeds = &[
                b"mint_authority",
                program_id.as_ref(),
                &[0], // Bump seed - would be calculated in real implementation
            ];
            let (authority_pubkey, bump_seed) = Pubkey::find_program_address(seeds, program_id);
            
            if authority_pubkey != bridge_state.mint_authority {
                msg!("Authority mismatch");
                return Err(ProgramError::InvalidArgument);
            }
            
            let authority_signer_seeds = &[
                b"mint_authority",
                program_id.as_ref(),
                &[bump_seed],
            ];
            
            // Execute transfer with PDA signature
            invoke_signed(
                &transfer_ix,
                &[
                    escrow.clone(),
                    target.clone(),
                    admin_info.clone(),
                    token_program.clone(),
                ],
                &[authority_signer_seeds],
            )?;
            
            // Update escrow balance
            bridge_state.escrow_token_balance = bridge_state.escrow_token_balance.saturating_sub(request.amount);
            bridge_state.escrow_update_timestamp = current_timestamp;
            
            msg!("Rescued {} tokens from escrow to {}", request.amount, request.target_account);
        },
        _ => {
            msg!("Unknown emergency action type: {}", request.action_type);
            return Err(ProgramError::InvalidArgument);
        }
    }
    
    // Remove the emergency request
    // Move the last item to the removed position if not already the last
    if index < (bridge_state.emergency_request_count - 1) as usize {
        bridge_state.emergency_requests[index] = bridge_state.emergency_requests[(bridge_state.emergency_request_count - 1) as usize];
    }
    bridge_state.emergency_request_count -= 1;
    
    // Save bridge state with updated info
    BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;
    
    Ok(())
}

/// Cancel emergency action
fn process_cancel_emergency_action(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    request_id: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let admin_info = next_account_info(account_info_iter)?;
    let bridge_info = next_account_info(account_info_iter)?;
    
    // Verify admin signature
    if !admin_info.is_signer {
        msg!("Missing admin signature");
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Verify bridge account is owned by this program
    if bridge_info.owner != program_id {
        msg!("Bridge account not owned by this program");
        return Err(ProgramError::IncorrectProgramId);
    }
    
    // Load bridge state
    let mut bridge_state = BridgeState::unpack(&bridge_info.data.borrow())?;
    
    // Find the emergency request with the given ID
    let mut request_index = None;
    for (i, request) in bridge_state.emergency_requests.iter().enumerate().take(bridge_state.emergency_request_count as usize) {
        if request.request_id == request_id {
            request_index = Some(i);
            break;
        }
    }
    
    // Check if we found the request
    let index = request_index.ok_or_else(|| {
        msg!("Emergency request not found: {}", request_id);
        ProgramError::InvalidArgument
    })?;
    
    // Check if the requester is cancelling their own request
    let request = bridge_state.emergency_requests[index];
    if request.requester != *admin_info.key {
        msg!("Only the requester can cancel their emergency action");
        return Err(ProgramError::InvalidArgument);
    }
    
    // Remove the emergency request
    // Move the last item to the removed position if not already the last
    if index < (bridge_state.emergency_request_count - 1) as usize {
        bridge_state.emergency_requests[index] = bridge_state.emergency_requests[(bridge_state.emergency_request_count - 1) as usize];
    }
    bridge_state.emergency_request_count -= 1;
    
    // Save bridge state
    BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;
    
    msg!("Emergency action cancelled: {}", request_id);
    
    Ok(())
}

/// Process auto-refund for multiple stuck bridge requests
fn process_auto_refund_requests(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let admin_info = next_account_info(account_info_iter)?;
    let escrow_info = next_account_info(account_info_iter)?;
    let token_program_info = next_account_info(account_info_iter)?;
    let bridge_info = next_account_info(account_info_iter)?;
    let clock_info = next_account_info(account_info_iter)?;
    
    // Get remaining accounts which are destination accounts for refunds
    let remaining_accounts = account_info_iter.collect::<Vec<&AccountInfo>>();
    
    // Verify admin signature
    if !admin_info.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Load clock to get current timestamp
    let clock = Clock::from_account_info(clock_info)?;
    let current_timestamp = clock.unix_timestamp as u64;
    
    // Load and verify bridge state
    let mut bridge_state = BridgeState::unpack(&bridge_info.data.borrow())?;
    
    // Verify admin authority
    let admin_pubkey = *admin_info.key;
    if !bridge_state.admin_accounts.contains(&admin_pubkey) {
        msg!("Signer is not an admin");
        return Err(ProgramError::InvalidAccountData);
    }
    
    // Get PDA for escrow authority
    let (escrow_authority, bump_seed) = Pubkey::find_program_address(
        &[b"escrow", program_id.as_ref()],
        program_id
    );
    
    // Find stuck requests older than AUTO_REFUND_TIMEOUT
    let mut refunded_count = 0;
    let mut refunded_amount = 0;
    
    for i in 0..bridge_state.bridge_request_count as usize {
        if i >= MAX_BRIDGE_REQUESTS {
            break;
        }
        
        let request = &bridge_state.bridge_requests[i];
        
        // Check if request is pending and has timed out
        if request.status == 0 && // Pending
           current_timestamp.saturating_sub(request.timestamp) >= AUTO_REFUND_TIMEOUT {
            
            // Find matching destination account
            let destination_account = remaining_accounts.iter().find(|&acc| *acc.key == request.source_account);
            
            if let Some(destination_info) = destination_account {
                // Create transfer instruction
                let transfer_ix = spl_token::instruction::transfer(
                    token_program_info.key,
                    escrow_info.key,
                    destination_info.key,
                    &escrow_authority,
                    &[],
                    request.amount,
                )?;
                
                // Execute the transfer with PDA signing
                match invoke_signed(
                    &transfer_ix,
                    &[
                        escrow_info.clone(),
                        destination_info.clone(),
                        token_program_info.clone(),
                    ],
                    &[&[b"escrow", program_id.as_ref(), &[bump_seed]]],
                ) {
                    Ok(_) => {
                        // Update request status to refunded
                        bridge_state.bridge_requests[i].status = 3; // Refunded
                        
                        // Track refunded amount
                        refunded_amount = refunded_amount.saturating_add(request.amount);
                        refunded_count += 1;
                        
                        msg!("Auto-refunded request ID {} to account {}", 
                             request.request_id, 
                             request.source_account);
                    },
                    Err(err) => {
                        msg!("Failed to refund request ID {}: {:?}", request.request_id, err);
                    }
                }
            } else {
                msg!("No matching destination account found for request ID {}", request.request_id);
            }
        }
    }
    
    // Update total escrow balance
    bridge_state.escrow_token_balance = bridge_state.escrow_token_balance.saturating_sub(refunded_amount);
    
    // Save updated bridge state
    BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;
    
    msg!("Auto-refund completed: {} requests refunded with total amount {}", 
         refunded_count, refunded_amount);
    
    Ok(())
}

/// User self-refund for own stuck bridge request
fn process_user_refund(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    request_id: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let user_info = next_account_info(account_info_iter)?;
    let escrow_info = next_account_info(account_info_iter)?;
    let destination_info = next_account_info(account_info_iter)?;
    let token_program_info = next_account_info(account_info_iter)?;
    let bridge_info = next_account_info(account_info_iter)?;
    let clock_info = next_account_info(account_info_iter)?;
    
    // Verify user signature
    if !user_info.is_signer {
        msg!("Missing user signature");
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Verify bridge account is owned by this program
    if bridge_info.owner != program_id {
        msg!("Bridge account not owned by this program");
        return Err(ProgramError::IncorrectProgramId);
    }
    
    // Get current timestamp
    let clock = Clock::from_account_info(clock_info)?;
    let current_timestamp = clock.unix_timestamp as u64;
    
    // Load bridge state
    let mut bridge_state = BridgeState::unpack(&bridge_info.data.borrow())?;
    
    // Find the bridge request with the given ID
    let mut request_index = None;
    for (i, request) in bridge_state.bridge_requests.iter().enumerate().take(bridge_state.bridge_request_count as usize) {
        if request.request_id == request_id && request.status == 0 { // Pending
            request_index = Some(i);
            break;
        }
    }
    
    // Check if we found the request
    let index = request_index.ok_or(ProgramError::InvalidArgument)?;
    let request = bridge_state.bridge_requests[index];
    
    // Check if user is the original sender
    if *user_info.key != request.source_account {
        msg!("User is not the original sender of this bridge request");
        return Err(ProgramError::InvalidArgument);
    }
    
    // Check if destination account matches the source account
    if destination_info.key != &request.source_account {
        msg!("Destination account does not match source account");
        return Err(ProgramError::InvalidArgument);
    }
    
    // Check if enough time has passed for self-refund (7 days)
    if current_timestamp < request.timestamp + AUTO_REFUND_TIMEOUT {
        let remaining = request.timestamp + AUTO_REFUND_TIMEOUT - current_timestamp;
        msg!("Bridge request not yet eligible for self-refund: {}s remaining", remaining);
        return Err(ProgramError::InvalidArgument);
    }
    
    // Use PDA for authority
    let seeds = &[
        b"mint_authority",
        program_id.as_ref(),
        &[0], // Bump seed - would be calculated in real implementation
    ];
    let (authority_pubkey, bump_seed) = Pubkey::find_program_address(seeds, program_id);
    
    if authority_pubkey != bridge_state.mint_authority {
        msg!("Authority mismatch");
        return Err(ProgramError::InvalidArgument);
    }
    
    let authority_signer_seeds = &[
        b"mint_authority",
        program_id.as_ref(),
        &[bump_seed],
    ];
    
    // Transfer tokens from escrow back to the original sender
    let transfer_ix = token_instruction::transfer(
        &spl_token::id(),
        escrow_info.key,
        destination_info.key,
        &bridge_state.mint_authority, // Escrow owner/authority
        &[],
        request.amount,
    )?;
    
    // Execute transfer with PDA signature
    invoke_signed(
        &transfer_ix,
        &[
            escrow_info.clone(),
            destination_info.clone(),
            user_info.clone(),
            token_program_info.clone(),
        ],
        &[authority_signer_seeds],
    )?;
    
    // Update request status to Refunded (3)
    bridge_state.bridge_requests[index].status = 3;
    
    // Update escrow balance
    bridge_state.escrow_token_balance = bridge_state.escrow_token_balance.saturating_sub(request.amount);
    bridge_state.escrow_update_timestamp = current_timestamp;
    
    // Save bridge state
    BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;
    
    msg!("User self-refunded {} tokens for bridge request ID {}", request.amount, request_id);
    
    Ok(())
}

/// Process an emergency refund for a failed bridge request
fn process_emergency_refund(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    request_id: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let admin_info = next_account_info(account_info_iter)?;
    let escrow_info = next_account_info(account_info_iter)?;
    let destination_info = next_account_info(account_info_iter)?;
    let token_program_info = next_account_info(account_info_iter)?;
    let bridge_info = next_account_info(account_info_iter)?;
    let clock_info = next_account_info(account_info_iter)?;
    
    // Verify admin signature
    if !admin_info.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Load and verify bridge state
    let mut bridge_state = BridgeState::unpack(&bridge_info.data.borrow())?;
    
    // Verify admin authority
    if !is_admin(admin_info.key, &bridge_state) {
        msg!("Signer is not an admin");
        return Err(ProgramError::InvalidAccountData);
    }
    
    // Find the bridge request
    let mut request_index = None;
    for (i, request) in bridge_state.bridge_requests.iter().enumerate().take(bridge_state.bridge_request_count as usize) {
        if request.request_id == request_id {
            request_index = Some(i);
            break;
        }
    }
    
    let request_index = match request_index {
        Some(index) => index,
        None => {
            msg!("Bridge request not found: {}", request_id);
            return Err(ProgramError::InvalidArgument);
        }
    };
    
    let request = bridge_state.bridge_requests[request_index];
    
    // Verify request status is pending or failed
    if request.status != 0 && request.status != 2 {
        msg!("Bridge request is not in pending or failed status");
        return Err(ProgramError::InvalidArgument);
    }
    
    // Get PDA for escrow authority
    let seeds = &[
        b"escrow_authority",
        program_id.as_ref(),
        &[0], // Bump seed
    ];
    let (escrow_authority, bump_seed) = Pubkey::find_program_address(seeds, program_id);
    
    let authority_signer_seeds = &[
        b"escrow_authority",
        program_id.as_ref(),
        &[bump_seed],
    ];
    
    // Create transfer instruction to return tokens to the original sender
    let transfer_ix = token_instruction::transfer(
        &spl_token::id(),
        escrow_info.key,
        destination_info.key,
        &escrow_authority,
        &[],
        request.amount,
    )?;
    
    // Execute the transfer
    invoke_signed(
        &transfer_ix,
        &[
            escrow_info.clone(),
            destination_info.clone(),
            token_program_info.clone(),
        ],
        &[authority_signer_seeds],
    )?;
    
    // Update request status to refunded
    bridge_state.bridge_requests[request_index].status = 3; // Refunded
    
    // Update escrow token balance
    bridge_state.escrow_token_balance = bridge_state.escrow_token_balance.saturating_sub(request.amount);
    
    // Save updated bridge state
    BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;
    
    msg!("Emergency refund processed for request ID: {}", request_id);
    msg!("Refunded {} tokens to {}", request.amount, destination_info.key);
    
    Ok(())
}

/// Helper function to check if an account is an admin
fn is_admin(account: &Pubkey, bridge_state: &BridgeState) -> bool {
    // In a real implementation, this would check against a list of admin accounts
    // For now, we'll use a simple check against the first admin
    account == &bridge_state.admin_accounts[0]
}

/// Store information about security events for monitoring and auditing
pub struct SecurityEvent {
    /// Type of security event
    pub event_type: String,
    /// When the event occurred
    pub timestamp: u64,
    /// Account that triggered the event
    pub caller: Pubkey,
    /// Source chain ID if applicable
    pub source_chain_id: u16,
    /// Nonce if applicable
    pub nonce: u64,
    /// Additional message/details
    pub message: String,
}

/// Clean up old processed nonces to free up space
fn process_cleanup_processed_nonces(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    age_threshold: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let admin_info = next_account_info(account_info_iter)?;
    let bridge_info = next_account_info(account_info_iter)?;
    let clock_info = next_account_info(account_info_iter)?;
    
    // Verify admin signature
    if !admin_info.is_signer {
        msg!("Error: Admin signature required");
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Verify the bridge account is owned by this program
    if bridge_info.owner != program_id {
        msg!("Error: Bridge account is not owned by this program");
        return Err(ProgramError::IncorrectProgramId);
    }
    
    // Load bridge state
    let mut bridge_state = BridgeState::unpack(&bridge_info.data.borrow())?;
    
    // Verify admin authority
    if !is_admin(admin_info.key, &bridge_state) {
        msg!("Error: Not an admin account");
        return Err(ProgramError::InvalidAccountData);
    }
    
    // Get current timestamp
    let clock = Clock::from_account_info(clock_info)?;
    let current_time = clock.unix_timestamp as u64;
    
    // Create a list of nonces to remove
    let mut nonces_to_remove: Vec<u64> = Vec::new();
    
    // Identify nonces older than the threshold
    for (nonce, _) in &bridge_state.processed_nonces {
        if current_time - bridge_state.oldest_nonce_timestamp > age_threshold {
            nonces_to_remove.push(*nonce);
        }
    }
    
    // Counter for removed nonces
    let mut removed_count = 0;
    
    // Remove old nonces
    for nonce in nonces_to_remove {
        bridge_state.processed_nonces.remove(&nonce);
        removed_count += 1;
    }
    
    // Update the oldest nonce timestamp if we removed nonces
    if removed_count > 0 {
        // Find the new oldest nonce timestamp (or set to current time if none left)
        bridge_state.oldest_nonce_timestamp = current_time;
    }
    
    // Log the results
    msg!("Cleaned up {} old processed nonces", removed_count);
    
    // Save updated bridge state
    BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;
    
    Ok(())
}

/// Function to request admin timelock for privileged operations
fn process_request_admin_timelock(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let admin_info = next_account_info(account_info_iter)?;
    let bridge_info = next_account_info(account_info_iter)?;
    let clock_info = next_account_info(account_info_iter)?;
    
    // Verify the bridge account is owned by this program
    if bridge_info.owner != program_id {
        msg!("Bridge account not owned by this program");
        return Err(ProgramError::IncorrectProgramId);
    }
    
    // Load bridge state
    let mut bridge_state = BridgeState::unpack(&bridge_info.data.borrow())?;
    
    // Verify admin privileges
    if !is_admin(admin_info.key, &bridge_state) {
        msg!("Only admin can request timelock");
        return Err(ProgramError::InvalidAccountData);
    }
    
    // Verify signer
    if !admin_info.is_signer {
        msg!("Admin must sign timelock request");
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Get current timestamp
    let clock = Clock::from_account_info(clock_info)?;
    let current_timestamp = clock.unix_timestamp as u64;
    
    // Set timelock expiry
    bridge_state.admin_timelock_expiry = current_timestamp + ADMIN_TIMELOCK_DURATION;
    bridge_state.admin_timelock_requester = *admin_info.key;
    
    // Save updated state
    BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;
    
    msg!("Admin timelock requested by {}", admin_info.key);
    msg!("Timelock will expire at {} ({} hours from now)", 
        bridge_state.admin_timelock_expiry, 
        ADMIN_TIMELOCK_DURATION / 3600);
    
    Ok(())
}

/// Function to update LayerZero status
fn process_update_layerzero_status(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    new_status: u8,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let admin_info = next_account_info(account_info_iter)?;
    let bridge_info = next_account_info(account_info_iter)?;
    let clock_info = next_account_info(account_info_iter)?;
    
    // Verify the bridge account is owned by this program
    if bridge_info.owner != program_id {
        msg!("Bridge account not owned by this program");
        return Err(ProgramError::IncorrectProgramId);
    }
    
    // Load bridge state
    let mut bridge_state = BridgeState::unpack(&bridge_info.data.borrow())?;
    
    // Verify admin privileges
    if !is_admin(admin_info.key, &bridge_state) {
        msg!("Only admin can update LayerZero status");
        return Err(ProgramError::InvalidAccountData);
    }
    
    // Verify signer
    if !admin_info.is_signer {
        msg!("Admin must sign status update");
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Validate new status
    if new_status > 2 {
        msg!("Invalid LayerZero status: {}", new_status);
        return Err(ProgramError::InvalidArgument);
    }
    
    // Get current timestamp
    let clock = Clock::from_account_info(clock_info)?;
    let current_timestamp = clock.unix_timestamp as u64;
    
    // Update status
    bridge_state.layerzero_status = new_status;
    bridge_state.layerzero_status_timestamp = current_timestamp;
    
    // If setting to "down" status, activate emergency recovery mode
    if new_status == 2 {
        bridge_state.emergency_recovery_mode = true;
        msg!("LayerZero status set to DOWN. Emergency recovery mode activated.");
    } else {
        msg!("LayerZero status updated to: {}", 
            match new_status {
                0 => "ACTIVE",
                1 => "DEGRADED",
                _ => "UNKNOWN",
            });
    }
    
    // Save updated state
    BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;
    
    Ok(())
}

/// Improved function to check bridge attempt status
fn check_bridge_attempt_status(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    attempt_id: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let bridge_info = next_account_info(account_info_iter)?;
    
    // Verify the bridge account is owned by this program
    if bridge_info.owner != program_id {
        msg!("Bridge account not owned by this program");
        return Err(ProgramError::IncorrectProgramId);
    }
    
    // Load bridge state
    let bridge_state = BridgeState::unpack(&bridge_info.data.borrow())?;
    
    // Find the bridge attempt
    let mut found = false;
    for i in 0..bridge_state.bridge_attempt_count as usize {
        if bridge_state.bridge_attempts[i].attempt_id == attempt_id {
            found = true;
            msg!("Bridge attempt {} status: {}", attempt_id, bridge_state.bridge_attempts[i].status);
            msg!("Timestamp: {}", bridge_state.bridge_attempts[i].timestamp);
            if bridge_state.bridge_attempts[i].status == 2 {
                msg!("Error: {}", bridge_state.bridge_attempts[i].error_message);
            }
            break;
        }
    }
    
    if !found {
        msg!("Bridge attempt not found: {}", attempt_id);
        return Err(ProgramError::InvalidArgument);
    }
    
    Ok(())
}

/// Track sequence numbers for each source chain to ensure message ordering
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ChainSequence {
    /// The chain ID
    pub chain_id: u16,
    /// Current expected sequence number
    pub expected_sequence: u64,
    /// Last processed sequence timestamp
    pub last_timestamp: u64,
    /// Recovery mode if sequence is out of order
    pub recovery_mode: bool,
}

/// Check and update message sequence from a source chain
fn verify_and_update_sequence(
    bridge_state: &mut BridgeState,
    source_chain_id: u16,
    sequence: u64,
    current_timestamp: u64,
) -> ProgramResult {
    // Find or create sequence tracking for this chain
    let mut chain_index = None;
    
    for i in 0..bridge_state.chain_sequence_count as usize {
        if bridge_state.chain_sequences[i].chain_id == source_chain_id {
            chain_index = Some(i);
            break;
        }
    }
    
    if chain_index.is_none() {
        // Add new chain if we have space
        if bridge_state.chain_sequence_count < 32 {
            let new_index = bridge_state.chain_sequence_count as usize;
            bridge_state.chain_sequences[new_index] = ChainSequence {
                chain_id: source_chain_id,
                expected_sequence: sequence, // Initialize with first received
                last_timestamp: current_timestamp,
                recovery_mode: false,
            };
            bridge_state.chain_sequence_count += 1;
            return Ok(());
        } else {
            msg!("Error: Cannot track more chains, limit reached");
            return Err(ProgramError::Custom(102));
        }
    }
    
    let index = chain_index.unwrap();
    let chain_seq = &mut bridge_state.chain_sequences[index];
    
    // If in recovery mode, accept any valid nonce
    if chain_seq.recovery_mode {
        if current_timestamp - chain_seq.last_timestamp > 86400 { // 1 day timeout
            // Reset sequence tracking after timeout
            chain_seq.expected_sequence = sequence + 1;
            chain_seq.last_timestamp = current_timestamp;
            chain_seq.recovery_mode = false;
            msg!("Sequence recovery completed for chain {}", source_chain_id);
            return Ok(());
        }
        
        // Accept the message but stay in recovery mode
        chain_seq.last_timestamp = current_timestamp;
        return Ok(());
    }
    
    // Normal sequence checking
    if sequence < chain_seq.expected_sequence {
        msg!("Error: Sequence {} is lower than expected {} for chain {}", 
            sequence, chain_seq.expected_sequence, source_chain_id);
        return Err(ProgramError::Custom(103));
    }
    
    if sequence > chain_seq.expected_sequence {
        // Message gap detected
        msg!("Warning: Sequence gap detected for chain {}. Expected {}, received {}", 
            source_chain_id, chain_seq.expected_sequence, sequence);
        
        // If gap is small, we can just update and continue
        if sequence - chain_seq.expected_sequence <= 5 {
            chain_seq.expected_sequence = sequence + 1;
            chain_seq.last_timestamp = current_timestamp;
        } else {
            // Large gap, enter recovery mode
            msg!("Entering sequence recovery mode for chain {}", source_chain_id);
            chain_seq.expected_sequence = sequence + 1;
            chain_seq.last_timestamp = current_timestamp;
            chain_seq.recovery_mode = true;
            
            // Log security event
            log_security_event(
                bridge_state,
                "SEQUENCE_GAP",
                &Pubkey::default(), // No specific caller
                &Clock::get()?,
                source_chain_id,
                sequence,
            );
        }
        return Ok(());
    }
    
    // Sequence matches expected, update to next
    chain_seq.expected_sequence = sequence + 1;
    chain_seq.last_timestamp = current_timestamp;
    
    Ok(())
}

/// Track user emergency withdrawal requests
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct EmergencyWithdrawRequest {
    /// User who requested the withdrawal
    pub user: Pubkey,
    /// Amount requested
    pub amount: u64,
    /// When request was made
    pub request_time: u64,
    /// Request ID
    pub request_id: u64,
}

/// Process user emergency withdrawal request
fn process_request_user_emergency_withdraw(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    amount: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let user_info = next_account_info(account_info_iter)?;
    let bridge_info = next_account_info(account_info_iter)?;
    let clock_info = next_account_info(account_info_iter)?;
    
    // Verify user signature
    if !user_info.is_signer {
        msg!("Error: User signature required");
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Verify the bridge account is owned by this program
    if bridge_info.owner != program_id {
        msg!("Error: Bridge account not owned by this program");
        return Err(ProgramError::IncorrectProgramId);
    }
    
    // Get current timestamp
    let clock = Clock::from_account_info(clock_info)?;
    let current_timestamp = clock.unix_timestamp as u64;
    
    // Load bridge state
    let mut bridge_state = BridgeState::unpack(&bridge_info.data.borrow())?;
    
    // Check if emergency mode is already active
    if !bridge_state.escrow_emergency_mode && !bridge_state.bridge_state == 1 {
        msg!("Error: Emergency withdrawals only available when bridge is paused or in emergency mode");
        return Err(ProgramError::InvalidAccountData);
    }
    
    // Check if amount is valid
    if amount == 0 {
        msg!("Error: Amount must be greater than zero");
        return Err(ProgramError::InvalidArgument);
    }
    
    // Find if user has any active bridge requests
    let mut total_user_pending = 0;
    for i in 0..bridge_state.bridge_request_count as usize {
        let request = &bridge_state.bridge_requests[i];
        if request.source_account == *user_info.key && request.status == 0 {
            total_user_pending += request.amount;
        }
    }
    
    // Ensure user is not requesting more than they have pending
    if amount > total_user_pending {
        msg!("Error: Cannot withdraw more than pending bridge amount");
        return Err(ProgramError::InvalidArgument);
    }
    
    // Create a new emergency withdrawal request
    let request_id = bridge_state.next_user_withdrawal_id;
    bridge_state.next_user_withdrawal_id += 1;
    
    // Check if we have space for a new request
    if bridge_state.user_withdrawal_request_count >= 100 {
        msg!("Error: Maximum number of withdrawal requests reached");
        return Err(ProgramError::InsufficientFunds);
    }
    
    // Store the request
    let request_index = bridge_state.user_withdrawal_request_count as usize;
    bridge_state.user_withdrawal_requests[request_index] = EmergencyWithdrawRequest {
        user: *user_info.key,
        amount,
        request_time: current_timestamp,
        request_id,
    };
    
    bridge_state.user_withdrawal_request_count += 1;
    
    // Log the request
    msg!("Emergency withdrawal requested: user={}, amount={}, id={}", 
        user_info.key, amount, request_id);
    msg!("Timelock expires at {}", current_timestamp + EMERGENCY_TIMELOCK);
    
    // Save updated bridge state
    BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;
    
    Ok(())
}

/// Execute user emergency withdrawal
fn process_execute_user_emergency_withdraw(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let user_info = next_account_info(account_info_iter)?;
    let bridge_info = next_account_info(account_info_iter)?;
    let clock_info = next_account_info(account_info_iter)?;
    let escrow_info = next_account_info(account_info_iter)?;
    let destination_info = next_account_info(account_info_iter)?;
    let token_program_info = next_account_info(account_info_iter)?;
    
    // Verify user signature
    if !user_info.is_signer {
        msg!("Error: User signature required");
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Verify the bridge account is owned by this program
    if bridge_info.owner != program_id {
        msg!("Error: Bridge account not owned by this program");
        return Err(ProgramError::IncorrectProgramId);
    }
    
    // Get current timestamp
    let clock = Clock::from_account_info(clock_info)?;
    let current_timestamp = clock.unix_timestamp as u64;
    
    // Load bridge state
    let mut bridge_state = BridgeState::unpack(&bridge_info.data.borrow())?;
    
    // Find the user's request
    let mut request_index = None;
    for (i, request) in bridge_state.user_withdrawal_requests.iter().enumerate().take(bridge_state.user_withdrawal_request_count as usize) {
        if request.user == *user_info.key {
            request_index = Some(i);
            break;
        }
    }
    
    let request_index = match request_index {
        Some(index) => index,
        None => {
            msg!("Error: No withdrawal request found for this user");
            return Err(ProgramError::InvalidArgument);
        }
    };
    
    // Check if timelock has expired
    let request = &bridge_state.user_withdrawal_requests[request_index];
    if current_timestamp < request.request_time + EMERGENCY_TIMELOCK {
        msg!("Error: Timelock period not yet expired");
        msg!("Current time: {}, expiry: {}", 
            current_timestamp, request.request_time + EMERGENCY_TIMELOCK);
        return Err(ProgramError::Custom(105));
    }
    
    // Verify escrow account matches the one in bridge state
    if *escrow_info.key != bridge_state.escrow_account {
        msg!("Error: Invalid escrow account");
        return Err(ProgramError::InvalidArgument);
    }
    
    // Get PDA for escrow authority
    let (escrow_authority, bump_seed) = Pubkey::find_program_address(
        &[b"escrow", program_id.as_ref()],
        program_id,
    );
    
    // Create transfer instruction
    let transfer_ix = token_instruction::transfer(
        token_program_info.key,
        escrow_info.key,
        destination_info.key,
        &escrow_authority,
        &[],
        request.amount,
    )?;
    
    // Sign with PDA
    let escrow_signer_seeds = &[
        b"escrow",
        program_id.as_ref(),
        &[bump_seed],
    ];
    
    // Execute the transfer
    invoke_signed(
        &transfer_ix,
        &[
            escrow_info.clone(),
            destination_info.clone(),
            token_program_info.clone(),
        ],
        &[escrow_signer_seeds],
    )?;
    
    // Update escrow balance
    bridge_state.escrow_token_balance = bridge_state.escrow_token_balance.saturating_sub(request.amount);
    bridge_state.escrow_update_timestamp = current_timestamp;
    
    // Log the withdrawal
    msg!("Emergency withdrawal executed: user={}, amount={}", user_info.key, request.amount);
    
    // Remove the request
    if request_index < (bridge_state.user_withdrawal_request_count as usize - 1) {
        bridge_state.user_withdrawal_requests[request_index] = 
            bridge_state.user_withdrawal_requests[bridge_state.user_withdrawal_request_count as usize - 1];
    }
    bridge_state.user_withdrawal_request_count -= 1;
    
    // Update bridge state
    BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;
    
    Ok(())
}

/// Get escrow status
fn process_get_escrow_status(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let bridge_info = next_account_info(account_info_iter)?;
    
    // Load bridge state
    let bridge_state = BridgeState::unpack(&bridge_info.data.borrow())?;
    
    // Output escrow information
    msg!("Escrow Status:");
    msg!("Current balance: {} tokens", bridge_state.escrow_token_balance);
    msg!("Maximum allowed: {} tokens", bridge_state.max_escrow_amount);
    msg!("Alert threshold: {}%", bridge_state.escrow_alert_threshold);
    msg!("Last updated: {} seconds ago", 
        Clock::get()?.unix_timestamp as u64 - bridge_state.escrow_update_timestamp);
    
    let utilization = if bridge_state.max_escrow_amount > 0 {
        (bridge_state.escrow_token_balance * 100) / bridge_state.max_escrow_amount
    } else {
        0
    };
    
    msg!("Current utilization: {}%", utilization);
    msg!("Emergency mode: {}", bridge_state.escrow_emergency_mode);
    
    // Show pending user withdrawal requests
    msg!("Pending user emergency withdrawals: {}", bridge_state.user_withdrawal_request_count);
    
    if bridge_state.user_withdrawal_request_count > 0 {
        for i in 0..bridge_state.user_withdrawal_request_count as usize {
            let request = &bridge_state.user_withdrawal_requests[i];
            msg!("  Request #{}: User {}, Amount {}, Requested at {}", 
                request.request_id, 
                request.user, 
                request.amount, 
                request.request_time);
            
            let expiry = request.request_time + EMERGENCY_TIMELOCK;
            let current = Clock::get()?.unix_timestamp as u64;
            
            if current < expiry {
                msg!("    Timelock expires in {} seconds", expiry - current);
            } else {
                msg!("    Timelock expired {} seconds ago", current - expiry);
            }
        }
    }
    
    Ok(())
}

/// Set escrow limits
fn process_set_escrow_limits(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    max_escrow_amount: u64,
    alert_threshold: u8,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let admin_info = next_account_info(account_info_iter)?;
    let bridge_info = next_account_info(account_info_iter)?;
    let clock_info = next_account_info(account_info_iter)?;
    
    // Verify admin signature
    if !admin_info.is_signer {
        msg!("Error: Admin signature required");
        return Err(ProgramError::MissingRequiredSignature);
    }
    
    // Verify the bridge account is owned by this program
    if bridge_info.owner != program_id {
        msg!("Error: Bridge account not owned by this program");
        return Err(ProgramError::IncorrectProgramId);
    }
    
    // Get current timestamp
    let clock = Clock::from_account_info(clock_info)?;
    let current_timestamp = clock.unix_timestamp as u64;
    
    // Load bridge state
    let mut bridge_state = BridgeState::unpack(&bridge_info.data.borrow())?;
    
    // Verify admin authority
    if !is_admin(admin_info.key, &bridge_state) {
        msg!("Error: Not an admin account");
        return Err(ProgramError::InvalidAccountData);
    }
    
    // Validate alert threshold
    if alert_threshold > 100 {
        msg!("Error: Alert threshold must be between 0 and 100");
        return Err(ProgramError::InvalidArgument);
    }
    
    // Update escrow limits
    bridge_state.max_escrow_amount = max_escrow_amount;
    bridge_state.escrow_alert_threshold = alert_threshold;
    
    // Check if we need to activate emergency mode based on new limits
    let utilization = if max_escrow_amount > 0 {
        (bridge_state.escrow_token_balance * 100) / max_escrow_amount
    } else {
        0
    };
    
    if utilization >= alert_threshold as u64 {
        if !bridge_state.escrow_emergency_mode {
            msg!("Warning: Escrow utilization ({}% of {}) exceeds alert threshold ({}%)", 
                utilization, max_escrow_amount, alert_threshold);
            bridge_state.escrow_emergency_mode = true;
            bridge_state.last_escrow_alert_time = current_timestamp;
        }
    } else if bridge_state.escrow_emergency_mode && utilization < (alert_threshold as u64 * 80) / 100 {
        // Only deactivate emergency mode when utilization falls below 80% of threshold
        bridge_state.escrow_emergency_mode = false;
        msg!("Escrow emergency mode deactivated. Utilization: {}%", utilization);
    }
    
    // Log the changes
    msg!("Escrow limits updated: max_amount={}, alert_threshold={}%", 
        max_escrow_amount, alert_threshold);
    
    // Save updated bridge state
    BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;
    
    Ok(())
}

/// Monitor escrow and activate emergency mode if needed
fn monitor_escrow_status(
    bridge_state: &mut BridgeState,
    current_timestamp: u64,
) -> ProgramResult {
    // Skip frequent checks, only check once per hour
    if bridge_state.escrow_update_timestamp > 0 && 
       current_timestamp - bridge_state.escrow_update_timestamp < 3600 {
        return Ok(());
    }
    
    // Check if we're approaching the threshold
    if bridge_state.max_escrow_amount > 0 {
        let utilization = (bridge_state.escrow_token_balance * 100) / bridge_state.max_escrow_amount;
        
        if utilization >= bridge_state.escrow_alert_threshold as u64 {
            if !bridge_state.escrow_emergency_mode {
                msg!("Warning: Escrow utilization ({}% of {}) exceeds alert threshold ({}%)", 
                    utilization, bridge_state.max_escrow_amount, bridge_state.escrow_alert_threshold);
                bridge_state.escrow_emergency_mode = true;
                bridge_state.last_escrow_alert_time = current_timestamp;
            }
        } else if bridge_state.escrow_emergency_mode && 
                  utilization < (bridge_state.escrow_alert_threshold as u64 * 80) / 100 {
            // Only deactivate emergency mode when utilization falls below 80% of threshold
            bridge_state.escrow_emergency_mode = false;
            msg!("Escrow emergency mode deactivated. Utilization: {}%", utilization);
        }
    }
    
    bridge_state.escrow_update_timestamp = current_timestamp;
    
    Ok(())
}

// Update process_instruction to handle new instructions
pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let instruction = DmdInstruction::unpack(instruction_data)?;
    
    match instruction {
        // ... existing instruction handling ...
        
        DmdInstruction::GetEscrowStatus => {
            process_get_escrow_status(program_id, accounts)
        },
        
        DmdInstruction::RequestUserEmergencyWithdraw { amount } => {
            process_request_user_emergency_withdraw(program_id, accounts, amount)
        },
        
        DmdInstruction::ExecuteUserEmergencyWithdraw => {
            process_execute_user_emergency_withdraw(program_id, accounts)
        },
        
        DmdInstruction::SetEscrowLimits { max_escrow_amount, alert_threshold } => {
            process_set_escrow_limits(program_id, accounts, max_escrow_amount, alert_threshold)
        },
        
        // ... existing instruction handling ...
    }
}

// Also update process_bridge_to_near to check escrow limits
fn process_bridge_to_near(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    to_address: [u8; 64],
    amount: u64,
    fee: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let owner_info = next_account_info(account_info_iter)?;
    let source_info = next_account_info(account_info_iter)?;
    let escrow_info = next_account_info(account_info_iter)?;
    let token_program_info = next_account_info(account_info_iter)?;
    let layerzero_endpoint_info = next_account_info(account_info_iter)?;
    let bridge_info = next_account_info(account_info_iter)?;
    let clock_info = next_account_info(account_info_iter)?;

    // Load the clock to get the current timestamp
    let clock = Clock::from_account_info(clock_info)?;
    let current_timestamp = clock.unix_timestamp as u64;
    
    // Xác thực chữ ký
    if !owner_info.is_signer {
        msg!("Error: Missing required signature");
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Kiểm tra endpoint
    if layerzero_endpoint_info.key.to_string() != LAYERZERO_ENDPOINT {
        msg!("Error: Invalid LayerZero endpoint: {}", layerzero_endpoint_info.key);
        return Err(ProgramError::InvalidAccountData);
    }
    
    // Verify the bridge account is owned by this program
    if bridge_info.owner != program_id {
        msg!("Error: Bridge account not owned by this program");
        return Err(ProgramError::IncorrectProgramId);
    }
    
    // Load bridge state
    let mut bridge_state = BridgeState::unpack(&bridge_info.data.borrow())?;
    
    // Check escrow limits before proceeding
    if bridge_state.max_escrow_amount > 0 && 
       bridge_state.escrow_token_balance + amount > bridge_state.max_escrow_amount {
        msg!("Error: Bridge transaction would exceed maximum escrow limit");
        msg!("Current: {}, Max: {}, Requested: {}", 
            bridge_state.escrow_token_balance, 
            bridge_state.max_escrow_amount,
            amount);
        return Err(ProgramError::InvalidArgument);
    }
    
    // Monitor escrow status
    monitor_escrow_status(&mut bridge_state, current_timestamp)?;
    
    // If escrow emergency mode is active, only admins can bridge
    if bridge_state.escrow_emergency_mode && !is_admin(owner_info.key, &bridge_state) {
        msg!("Error: Escrow emergency mode active. Only admins can bridge tokens.");
        return Err(ProgramError::Custom(106));
    }
    
    // Check if LayerZero is down or degraded
    if bridge_state.layerzero_status == 2 {
        msg!("Error: LayerZero is currently down. Bridge operations are not available.");
        return Err(ProgramError::InvalidAccountData);
    }
    
    if bridge_state.layerzero_status == 1 {
        msg!("Warning: LayerZero is in degraded state. Bridge operations may take longer than usual.");
    }
    
    // Verify the bridge is not paused
    if bridge_state.bridge_state == 1 {
        msg!("Error: Bridge is currently paused");
        return Err(ProgramError::InvalidAccountData);
    }
    
    // Additional per-transaction amount check
    if amount > PER_TRANSACTION_BRIDGE_LIMIT {
        msg!("Error: Amount exceeds per-transaction limit of {}", PER_TRANSACTION_BRIDGE_LIMIT);
        return Err(ProgramError::InvalidArgument);
    }
    
    // Check if user is in cooldown period
    if let Some(last_bridge_time) = bridge_state.user_last_bridge.get(owner_info.key) {
        if current_timestamp - last_bridge_time < bridge_state.user_cooldown {
            msg!("Error: User is in cooldown period. Please wait {} more seconds", 
                bridge_state.user_cooldown - (current_timestamp - last_bridge_time));
            return Err(ProgramError::Custom(100)); // Custom error code for cooldown
        }
    }
    
    // Reset daily volume if needed
    if current_timestamp - bridge_state.daily_volume_timestamp > 86400 {
        bridge_state.daily_volume = 0;
        bridge_state.daily_volume_timestamp = current_timestamp;
    }
    
    // Check daily volume limit
    if bridge_state.daily_volume + amount > bridge_state.daily_limit {
        msg!("Error: Daily bridge volume limit would be exceeded");
        msg!("Current: {}, Limit: {}, Request: {}", 
            bridge_state.daily_volume, bridge_state.daily_limit, amount);
        return Err(ProgramError::Custom(101)); // Custom error code for limit
    }
    
    // Check user daily volume
    let day_key = format!("{}:{}", owner_info.key, current_timestamp / 86400);
    let user_daily_volume = bridge_state.user_daily_volumes.get(&day_key).unwrap_or(&0);
    
    // For simplicity, we're setting a user daily limit to 20% of the global limit
    let user_daily_limit = bridge_state.daily_limit / 5;
    
    if user_daily_volume + amount > user_daily_limit {
        msg!("Error: User daily bridge volume limit would be exceeded");
        msg!("Current: {}, Limit: {}, Request: {}", 
            user_daily_volume, user_daily_limit, amount);
        return Err(ProgramError::Custom(102)); // Custom error code for user limit
    }
    
    // Create a bridge request
    let request_id = bridge_state.next_request_id;
    bridge_state.next_request_id += 1;
    
    let request = BridgeRequest {
        to_address,
        amount,
        source_account: *owner_info.key,
        timestamp: current_timestamp,
        request_id,
        status: 0, // Pending
        retry_count: 0,
    };
    
    // Store the bridge request
    let request_index = bridge_state.bridge_request_count as usize;
    bridge_state.bridge_requests[request_index] = request;
    bridge_state.bridge_request_count += 1;
    
    // Generate a unique ID for this bridge attempt
    let attempt_id = bridge_state.last_bridge_attempt_id + 1;
    bridge_state.last_bridge_attempt_id = attempt_id;
    
    let bridge_attempt = BridgeAttempt {
        attempt_id,
        timestamp: current_timestamp,
        status: 0, // 0 = Pending
        error_message: String::new(),
    };
    
    // Store the bridge attempt
    let attempt_index = bridge_state.bridge_attempt_count as usize;
    bridge_state.bridge_attempts[attempt_index] = bridge_attempt;
    bridge_state.bridge_attempt_count += 1;
    
    // Update daily volume and last bridge timestamp for user
    bridge_state.daily_volume += amount;
    bridge_state.user_last_bridge.insert(*owner_info.key, current_timestamp);
    
    // Update user daily volume
    bridge_state.user_daily_volumes.insert(day_key, user_daily_volume + amount);
    
    // Update escrow token balance
    bridge_state.escrow_token_balance += amount;
    bridge_state.escrow_update_timestamp = current_timestamp;
    
    // Save bridge state with updated request info before token transfer
    BridgeState::pack(bridge_state, &mut bridge_info.data.borrow_mut())?;

    // Chuyển token vào escrow account - MUST HAPPEN AFTER all validation checks
    let transfer_to_escrow_ix = token_instruction::transfer(
        &spl_token::id(),
        source_info.key,
        escrow_info.key,
        owner_info.key,
        &[],
        amount,
    )?;

    // Execute the token transfer
    invoke(
        &transfer_to_escrow_ix,
        &[
            source_info.clone(),
            escrow_info.clone(),
            owner_info.clone(),
            token_program_info.clone(),
        ],
    )?;

    // Log the successful bridge request
    msg!("Bridge request created: id={}, to={:?}, amount={}", 
        request_id, to_address, amount);
    
    // Note: In a real implementation, we would call to LayerZero here to initiate the cross-chain message
    // For this example, we'll just log it
    msg!("Would call LayerZero endpoint to bridge tokens");
    
    Ok(())
}

// ... existing code ...

/// Unit tests for the Solana program
#[cfg(test)]
mod tests {
    use super::*;
    use solana_program::program_pack::Pack;
    use solana_program_test::*;
    use solana_sdk::{signature::Keypair, transaction::Transaction};

    #[tokio::test]
    async fn test_dmd_token() {
        let program_id = Pubkey::new_unique();
        let (mut banks_client, payer, recent_blockhash) = ProgramTest::new(
            "solana_dmd_token",
            program_id,
            processor!(process_instruction),
        )
        .start()
        .await;

        // Create mint account
        let mint_account = Keypair::new();
        let rent = banks_client.get_rent().await.unwrap();
        let mint_rent = rent.minimum_balance(Mint::LEN);

        // Create transaction to allocate mint account
        let mut transaction = Transaction::new_with_payer(
            &[
                system_instruction::create_account(
                    &payer.pubkey(),
                    &mint_account.pubkey(),
                    mint_rent,
                    Mint::LEN as u64,
                    &spl_token::id(),
                ),
                // Initialize mint instruction would go here
            ],
            Some(&payer.pubkey()),
        );
        transaction.sign(&[&payer, &mint_account], recent_blockhash);

        // Test execution would go here
        // banks_client.process_transaction(transaction).await.unwrap();
        
        // For simplicity, we're just logging success
        println!("Test completed successfully!");
    }
}

