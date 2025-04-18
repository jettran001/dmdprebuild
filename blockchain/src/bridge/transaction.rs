//! Định nghĩa các cấu trúc giao dịch bridge

use crate::smartcontracts::dmd_token::DmdChain;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use ethers::types::U256;
use std::str::FromStr;

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
    /// Số lượng token (được lưu trữ dưới dạng Hex String để serialize)
    #[serde(with = "u256_string_serializer")]
    pub amount: U256,
    /// Token ID (đối với ERC1155/NFT)
    pub token_id: Option<String>,
    /// Phí bridge (được lưu trữ dưới dạng Hex String để serialize)
    #[serde(with = "u256_string_serializer")]
    pub fee: U256,
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

/// Module hỗ trợ serialize/deserialize U256 thành string và ngược lại
mod u256_string_serializer {
    use ethers::types::U256;
    use serde::{Deserialize, Deserializer, Serializer};
    use std::str::FromStr;

    pub fn serialize<S>(value: &U256, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format!("0x{:x}", value))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<U256, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let s = s.trim();
        
        // Hỗ trợ cả định dạng 0x-prefixed và không prefixed
        if s.starts_with("0x") {
            U256::from_str(s).map_err(serde::de::Error::custom)
        } else {
            // Thử chuyển đổi như một số thập phân
            U256::from_dec_str(s).map_err(serde::de::Error::custom)
        }
    }
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

        // Chuyển đổi amount và fee từ String sang U256
        let amount_u256 = if amount.starts_with("0x") {
            U256::from_str(&amount).unwrap_or(U256::zero())
        } else {
            U256::from_dec_str(&amount).unwrap_or(U256::zero())
        };

        let fee_u256 = if fee.starts_with("0x") {
            U256::from_str(&fee).unwrap_or(U256::zero())
        } else {
            U256::from_dec_str(&fee).unwrap_or(U256::zero())
        };

        Self {
            id,
            source_tx_id: None,
            hub_tx_id: None,
            target_tx_id: None,
            source_address,
            target_address,
            source_chain,
            target_chain,
            amount: amount_u256,
            token_id,
            fee: fee_u256,
            status: BridgeTransactionStatus::Initiated,
            created_at: now,
            updated_at: now,
            completed_at: None,
            metadata: HashMap::new(),
        }
    }

    /// Tạo giao dịch bridge mới với amount và fee là U256
    pub fn new_with_u256(
        id: String,
        source_address: String,
        target_address: String,
        source_chain: DmdChain,
        target_chain: DmdChain,
        amount: U256,
        token_id: Option<String>,
        fee: U256,
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

    /// Lấy amount dưới dạng String định dạng hex
    pub fn get_amount_hex(&self) -> String {
        format!("0x{:x}", self.amount)
    }

    /// Lấy amount dưới dạng String định dạng thập phân
    pub fn get_amount_decimal(&self) -> String {
        self.amount.to_string()
    }

    /// Lấy fee dưới dạng String định dạng hex
    pub fn get_fee_hex(&self) -> String {
        format!("0x{:x}", self.fee)
    }

    /// Lấy fee dưới dạng String định dạng thập phân
    pub fn get_fee_decimal(&self) -> String {
        self.fee.to_string()
    }

    /// Cập nhật trạng thái giao dịch
    pub fn update_status(&mut self, status: BridgeTransactionStatus) -> Result<(), String> {
        // Kiểm tra quy tắc chuyển đổi trạng thái
        match (&self.status, &status) {
            // Không cho phép chuyển từ trạng thái cuối (Completed, Failed) về trạng thái khác
            (BridgeTransactionStatus::Completed, _) => {
                return Err(format!(
                    "Không thể chuyển từ trạng thái Completed sang {:?}",
                    status
                ));
            }
            (BridgeTransactionStatus::Failed(_), _) => {
                return Err(format!(
                    "Không thể chuyển từ trạng thái Failed sang {:?}",
                    status
                ));
            }
            // Kiểm tra trình tự hợp lệ
            (BridgeTransactionStatus::Initiated, BridgeTransactionStatus::SourceConfirmed) => {}
            (BridgeTransactionStatus::Initiated, BridgeTransactionStatus::Failed(_)) => {}
            (BridgeTransactionStatus::SourceConfirmed, BridgeTransactionStatus::PendingOnHub) => {}
            (BridgeTransactionStatus::SourceConfirmed, BridgeTransactionStatus::Failed(_)) => {}
            (BridgeTransactionStatus::PendingOnHub, BridgeTransactionStatus::HubConfirmed) => {}
            (BridgeTransactionStatus::PendingOnHub, BridgeTransactionStatus::Failed(_)) => {}
            (BridgeTransactionStatus::HubConfirmed, BridgeTransactionStatus::PendingOnTarget) => {}
            (BridgeTransactionStatus::HubConfirmed, BridgeTransactionStatus::Failed(_)) => {}
            (BridgeTransactionStatus::PendingOnTarget, BridgeTransactionStatus::Completed) => {}
            (BridgeTransactionStatus::PendingOnTarget, BridgeTransactionStatus::Failed(_)) => {}
            // Trạng thái không thay đổi
            (current, new) if std::mem::discriminant(current) == std::mem::discriminant(new) => {}
            // Nếu không khớp với các luồng chuyển đổi trạng thái hợp lệ trên
            _ => {
                return Err(format!(
                    "Không cho phép chuyển từ trạng thái {:?} sang {:?}",
                    self.status, status
                ));
            }
        }

        // Cập nhật trạng thái
        self.status = status;
        self.updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Cập nhật thời gian hoàn thành nếu trạng thái là Completed
        if matches!(self.status, BridgeTransactionStatus::Completed) {
            self.completed_at = Some(self.updated_at);
        }

        Ok(())
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
    
    /// Lưu nhiều giao dịch cùng lúc
    fn save_batch(&self, transactions: &[BridgeTransaction]) -> Result<(), String> {
        // Triển khai mặc định: Lưu từng giao dịch một
        // Các lớp con nên ghi đè phương thức này để tối ưu hiệu suất
        for tx in transactions {
            self.save(tx)?;
        }
        Ok(())
    }
    
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
    
    /// Xóa giao dịch theo ID
    fn delete_by_id(&self, id: &str) -> Result<bool, String>;
    
    /// Xóa giao dịch cũ hơn thời gian cung cấp (unix timestamp)
    fn delete_older_than(&self, timestamp: u64) -> Result<usize, String>;
    
    /// Xóa các giao dịch đã hoàn thành hoặc thất bại cũ hơn thời gian cung cấp
    fn cleanup_completed_transactions(&self, older_than_timestamp: u64) -> Result<usize, String> {
        let completed_txs = self.find_by_status(&BridgeTransactionStatus::Completed)?;
        let mut failed_txs = vec![];
        
        // Tìm các giao dịch thất bại
        // Phải dùng cách tiếp cận này vì Failed có thêm thông tin lỗi
        let all_statuses = self.get_all_transactions()?;
        for tx in all_statuses {
            if let BridgeTransactionStatus::Failed(_) = tx.status {
                if tx.updated_at < older_than_timestamp {
                    failed_txs.push(tx);
                }
            }
        }
        
        // Lọc các giao dịch đã hoàn thành cũ hơn timestamp
        let old_completed_txs: Vec<_> = completed_txs
            .into_iter()
            .filter(|tx| tx.updated_at < older_than_timestamp)
            .collect();
        
        // Kết hợp các danh sách
        let old_txs: Vec<_> = old_completed_txs
            .into_iter()
            .chain(failed_txs.into_iter())
            .collect();
        
        // Xóa các giao dịch
        let mut deleted_count = 0;
        for tx in old_txs {
            if self.delete_by_id(&tx.id)? {
                deleted_count += 1;
            }
        }
        
        Ok(deleted_count)
    }
    
    /// Giới hạn số lượng giao dịch, giữ lại các giao dịch mới nhất
    fn limit_transaction_count(&self, max_count: usize) -> Result<usize, String>;
    
    /// Lấy tất cả các giao dịch
    fn get_all_transactions(&self) -> Result<Vec<BridgeTransaction>, String>;
    
    /// Đếm số lượng giao dịch
    fn count_transactions(&self) -> Result<usize, String> {
        let all_txs = self.get_all_transactions()?;
        Ok(all_txs.len())
    }
} 