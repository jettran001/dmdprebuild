//! # Module Bridge cho DiamondChain
//! 
//! Module này triển khai chức năng bridge token giữa các blockchain khác nhau.
//! Sử dụng mô hình hub and spoke với NEAR Protocol làm trung tâm.
//! 
//! ## Cấu trúc
//! 
//! * `bridge.rs`: Thực hiện chính cho bridge
//! * `manager.rs`: Quản lý các giao dịch bridge 
//! * `near_hub.rs`: Logic bridge với NEAR làm trung tâm
//! * `evm_spoke.rs`: Logic bridge từ/đến các EVM chains
//! * `transaction.rs`: Quản lý thông tin giao dịch bridge
//! * `error.rs`: Custom error types cho module bridge
//! * `types.rs`: Các kiểu dữ liệu chung cho module bridge
//! * `traits.rs`: Các trait chính định nghĩa giao diện bridge
//! * `oracle.rs`: Module oracle để theo dõi và đồng bộ dữ liệu giữa các blockchain
//! * `persistent_repository.rs`: Lưu trữ bền vững cho các giao dịch bridge

// External imports
use ethers::types::{Address, U256, H256};
use async_trait::async_trait;
use anyhow::Result;
use tracing::{info, debug, warn, error};

// Standard library imports
use std::str::FromStr;
use std::sync::Arc;

// Internal imports
use crate::smartcontracts::{
    TokenInterface,
    dmd_token::DmdChain,
    near_contract::NearContractProvider,
    bsc_contract::BscContractProvider,
    eth_contract::EthContractProvider,
    solana_contract::SolanaContractProvider,
    arb_contract::ArbContractProvider,
    polygon_contract::PolygonContractProvider,
};
use crate::oracle::DmdOracle;

// Các module con
mod bridge;
mod manager;
mod near_hub;
mod evm_spoke;
mod transaction;
mod types;
mod error;
mod traits;
mod oracle;
mod persistent_repository;

// Re-exports chính
pub use bridge::{BridgeManager, BridgeAdapter, LayerZeroAdapter, BridgeTransaction, BridgeTransactionStatus};
pub use manager::{
    BridgeManager as BridgeTransactionManager, 
    BridgeConfig, 
    BridgeResult as BridgeManagerResult,
};
pub use near_hub::{
    NearBridgeHub, 
    NearBridgeConfig, 
    NearBridgeError, 
    NearBridgeResult,
    NearMessageFormat,
};
pub use evm_spoke::{
    EvmBridgeSpoke, 
    EvmBridgeConfig, 
    EvmBridgeError, 
    EvmBridgeResult,
    EvmMessageRequest,
    EvmMessageResponse,
};
pub use transaction::{
    BridgeTransaction as DetailedBridgeTransaction, 
    BridgeTransactionRepository, 
    BridgeTransactionStatus as DetailedBridgeTransactionStatus,
};
pub use persistent_repository::{
    JsonBridgeTransactionRepository,
};
pub use types::{
    BridgeDirection, 
    BridgeTokenType, 
    BridgeStatus, 
    BridgeConfig,
    BridgeEvent,
    BridgeEventType,
    BridgeMetadata,
    BridgeRequestType,
    BridgeResponse,
    TokenAmount,
};
pub use error::{
    BridgeError, 
    BridgeResult, 
    is_bridge_supported, 
    is_evm_chain,
    BridgeTransactionNotFound,
    UnsupportedChain,
    UnsupportedRoute,
    InvalidAmount,
    InvalidAddress,
    InvalidStatus,
    TransactionFailed,
    ProviderError,
    SystemError,
    ConnectionError,
    TimeoutError,
    MissingProvider,
    ValidationError,
    TransactionCreationError,
    ConfigurationError,
};
pub use traits::{
    BridgeHub, 
    BridgeSpoke, 
    BridgeProvider,
    BridgeEventListener,
    BridgeEventEmitter,
    BridgeTransactionValidator,
    BridgeMessageEncoder,
    BridgeMessageDecoder,
};
pub use oracle::{
    OracleManager, 
    OracleProvider, 
    OracleData, 
    OracleDataType, 
    OracleUpdateStatus,
    ChainlinkOracle, 
    ChainlinkConfig,
    OracleConnectionPool,
    OraclePriceData,
    OracleEventData,
};

/// Cấu hình Bridge mặc định
#[derive(Debug, Clone)]
pub struct BridgeDefaultConfig {
    /// Số xác nhận cần thiết để xác minh giao dịch bridge
    pub verification_confirmations: u64,
    /// ID hợp đồng bridge trên NEAR
    pub near_hub_contract_id: String,
    /// Thời gian timeout mặc định cho giao dịch (ms)
    pub default_timeout_ms: u64,
    /// Số lượng giao dịch đang xử lý tối đa
    pub max_pending_transactions: usize,
    /// Thời gian tồn tại cache mặc định (ms)
    pub default_cache_ttl_ms: u64,
    /// Đường dẫn đến tệp lưu trữ giao dịch
    pub transaction_storage_path: String,
}

impl Default for BridgeDefaultConfig {
    fn default() -> Self {
        Self {
            verification_confirmations: 12,
            near_hub_contract_id: "dmd_bridge.near".to_string(),
            default_timeout_ms: 300_000, // 5 phút
            max_pending_transactions: 1_000,
            default_cache_ttl_ms: 60_000, // 1 phút
            transaction_storage_path: "./data/bridge_transactions.json".to_string(),
        }
    }
}

/// Cấu hình mặc định cho Bridge
pub static BRIDGE_CONFIG: once_cell::sync::Lazy<std::sync::RwLock<BridgeDefaultConfig>> = 
    once_cell::sync::Lazy::new(|| std::sync::RwLock::new(BridgeDefaultConfig::default()));

/// Oracle cache cho Bridge để theo dõi giao dịch cross-chain
pub static BRIDGE_ORACLE: once_cell::sync::Lazy<tokio::sync::RwLock<Option<std::sync::Arc<crate::oracle::DmdOracle>>>> = 
    once_cell::sync::Lazy::new(|| tokio::sync::RwLock::new(None));

/// Thiết lập cấu hình mặc định cho Bridge
pub fn set_bridge_config(config: BridgeDefaultConfig) -> Result<(), String> {
    BRIDGE_CONFIG.write()
        .map_err(|e| format!("Không thể lấy quyền ghi cho cấu hình bridge: {}", e))
        .map(|mut guard| *guard = config)
}

/// Lấy cấu hình mặc định cho Bridge
pub fn get_bridge_config() -> Result<BridgeDefaultConfig, String> {
    BRIDGE_CONFIG.read()
        .map_err(|e| format!("Không thể lấy quyền đọc cho cấu hình bridge: {}", e))
        .map(|guard| guard.clone())
}

/// Thiết lập Oracle cho Bridge
pub async fn set_bridge_oracle(oracle: std::sync::Arc<crate::oracle::DmdOracle>) {
    let mut oracle_guard = BRIDGE_ORACLE.write().await;
    *oracle_guard = Some(oracle);
}

/// Lấy instance của Oracle cho Bridge
pub async fn get_bridge_oracle() -> Option<std::sync::Arc<crate::oracle::DmdOracle>> {
    let oracle_guard = BRIDGE_ORACLE.read().await;
    oracle_guard.clone()
}

/// Khởi tạo và trả về repository lưu trữ giao dịch bridge
pub fn create_transaction_repository() -> anyhow::Result<Arc<dyn BridgeTransactionRepository + Send + Sync>> {
    match get_bridge_config() {
        Ok(config) => {
            let repo = JsonBridgeTransactionRepository::new(&config.transaction_storage_path)?
                .start_async_worker();
            Ok(Arc::new(repo))
        },
        Err(e) => Err(anyhow::anyhow!("Không thể lấy cấu hình bridge: {}", e))
    }
}

/// Xác thực dữ liệu bridge trước khi xử lý
pub fn validate_bridge_data(
    source_chain: &DmdChain,
    target_chain: &DmdChain,
    amount: &U256,
    source_address: &str,
    target_address: &str
) -> BridgeResult<()> {
    // Kiểm tra xem chuỗi nguồn và đích có được hỗ trợ không
    if !is_bridge_supported(source_chain, target_chain) {
        return Err(BridgeError::UnsupportedRoute(
            format!("Bridge không được hỗ trợ từ {:?} đến {:?}", source_chain, target_chain)
        ));
    }
    
    // Kiểm tra số lượng token
    if *amount == U256::zero() {
        return Err(BridgeError::InvalidAmount("Số lượng token phải lớn hơn 0".to_string()));
    }
    
    // Kiểm tra địa chỉ nguồn và đích
    if source_address.trim().is_empty() {
        return Err(BridgeError::InvalidAddress("Địa chỉ nguồn không được để trống".to_string()));
    }
    
    if target_address.trim().is_empty() {
        return Err(BridgeError::InvalidAddress("Địa chỉ đích không được để trống".to_string()));
    }
    
    Ok(())
} 