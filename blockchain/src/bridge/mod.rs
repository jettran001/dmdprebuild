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

// Re-exports chính
pub use bridge::{BridgeManager, BridgeAdapter, LayerZeroAdapter, BridgeTransaction, BridgeTransactionStatus};
pub use manager::BridgeManager as BridgeTransactionManager;
pub use near_hub::NearBridgeHub;
pub use evm_spoke::EvmBridgeSpoke;
pub use transaction::{BridgeTransaction as DetailedBridgeTransaction, BridgeTransactionRepository, BridgeTransactionStatus as DetailedBridgeTransactionStatus};
pub use types::{BridgeDirection, BridgeTokenType, BridgeStatus, BridgeConfig};
pub use error::{BridgeError, BridgeResult, is_bridge_supported, is_evm_chain};
pub use traits::{BridgeHub, BridgeSpoke, BridgeProvider};
pub use oracle::{
    OracleManager, OracleProvider, OracleData, OracleDataType, OracleUpdateStatus,
    ChainlinkOracle, ChainlinkConfig
};

// Các constants
pub const BRIDGE_VERIFICATION_CONFIRMATIONS: u64 = 12;
pub const NEAR_HUB_CONTRACT_ID: &str = "dmd_bridge.near";
pub const DEFAULT_BRIDGE_TIMEOUT_MS: u64 = 300000; // 5 phút
pub const MAX_PENDING_TRANSACTIONS: usize = 1000;
pub const DEFAULT_CACHE_TTL_MS: u64 = 60000; // 1 phút

/// Oracle cache cho Bridge để theo dõi giao dịch cross-chain
pub static BRIDGE_ORACLE: once_cell::sync::Lazy<tokio::sync::RwLock<Option<std::sync::Arc<crate::oracle::DmdOracle>>>> = 
    once_cell::sync::Lazy::new(|| tokio::sync::RwLock::new(None));

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