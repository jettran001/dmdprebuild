//! Định nghĩa các cấu trúc giao dịch bridge

use crate::smartcontracts::dmd_token::DmdChain;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Trạng thái của giao dịch bridge
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BridgeTransactionStatus {
    /// Giao dịch đã được bắt đầu trên chain nguồn
    Initiated,
    /// Giao dịch đã được xác nhận trên chain nguồn
    SourceConfirmed,
    /// Giao dịch đang chờ xử lý trên bridge (hub)
    PendingOnHub,
    /// Giao dịch đã được xác nhận trên hub
    HubConfirmed,
    /// Giao dịch đang chờ xử lý trên chain đích
    PendingOnTarget,
    /// Giao dịch đã hoàn thành
    Completed,
    /// Giao dịch đã thất bại
    Failed(String),
}

/// Cấu trúc giao dịch bridge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeTransaction {
    /// ID giao dịch bridge
    pub id: String,
    /// ID giao dịch trên chain nguồn
    pub source_tx_id: Option<String>,
    /// ID giao dịch trên hub
    pub hub_tx_id: Option<String>,
    /// ID giao dịch trên chain đích
    pub target_tx_id: Option<String>,
    /// Địa chỉ nguồn
    pub source_address: String,
    /// Địa chỉ đích
    pub target_address: String,
    /// Chain nguồn
    pub source_chain: DmdChain,
    /// Chain đích
    pub target_chain: DmdChain,
    /// Số lượng token
    pub amount: String,
    /// Token ID (đối với ERC1155/NFT)
    pub token_id: Option<String>,
    /// Phí bridge
    pub fee: String,
    /// Trạng thái giao dịch
    pub status: BridgeTransactionStatus,
    /// Thời gian khởi tạo (unix timestamp)
    pub created_at: u64,
    /// Thời gian cập nhật cuối (unix timestamp)
    pub updated_at: u64,
    /// Thời gian hoàn thành (unix timestamp)
    pub completed_at: Option<u64>,
    /// Dữ liệu bổ sung
    pub metadata: HashMap<String, String>,
}

impl BridgeTransaction {
    /// Tạo giao dịch bridge mới
    pub fn new(
        id: String,
        source_address: String,
        target_address: String,
        source_chain: DmdChain,
        target_chain: DmdChain,
        amount: String,
        token_id: Option<String>,
        fee: String,
    ) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            id,
            source_tx_id: None,
            hub_tx_id: None,
            target_tx_id: None,
            source_address,
            target_address,
            source_chain,
            target_chain,
            amount,
            token_id,
            fee,
            status: BridgeTransactionStatus::Initiated,
            created_at: now,
            updated_at: now,
            completed_at: None,
            metadata: HashMap::new(),
        }
    }

    /// Cập nhật trạng thái giao dịch
    pub fn update_status(&mut self, status: BridgeTransactionStatus) {
        self.status = status;
        self.updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if matches!(status, BridgeTransactionStatus::Completed) {
            self.completed_at = Some(self.updated_at);
        }
    }

    /// Thêm metadata
    pub fn add_metadata(&mut self, key: String, value: String) {
        self.metadata.insert(key, value);
        self.updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }
}

/// Kho lưu trữ giao dịch bridge
pub trait BridgeTransactionRepository {
    /// Lưu giao dịch
    fn save(&self, transaction: &BridgeTransaction) -> Result<(), String>;
    
    /// Tìm giao dịch theo ID
    fn find_by_id(&self, id: &str) -> Result<Option<BridgeTransaction>, String>;
    
    /// Tìm giao dịch theo ID trên chain nguồn
    fn find_by_source_tx_id(&self, tx_id: &str) -> Result<Option<BridgeTransaction>, String>;
    
    /// Tìm giao dịch theo ID trên hub
    fn find_by_hub_tx_id(&self, tx_id: &str) -> Result<Option<BridgeTransaction>, String>;
    
    /// Tìm giao dịch theo ID trên chain đích
    fn find_by_target_tx_id(&self, tx_id: &str) -> Result<Option<BridgeTransaction>, String>;
    
    /// Lấy danh sách giao dịch theo địa chỉ nguồn
    fn find_by_source_address(&self, address: &str) -> Result<Vec<BridgeTransaction>, String>;
    
    /// Lấy danh sách giao dịch theo địa chỉ đích
    fn find_by_target_address(&self, address: &str) -> Result<Vec<BridgeTransaction>, String>;
    
    /// Lấy danh sách giao dịch theo trạng thái
    fn find_by_status(&self, status: &BridgeTransactionStatus) -> Result<Vec<BridgeTransaction>, String>;
} 