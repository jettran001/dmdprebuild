//! Các kiểu dữ liệu chung cho module bridge

use serde::{Deserialize, Serialize};
use ethers::types::{Address, U256};
use chrono::{DateTime, Utc};

use crate::smartcontracts::dmd_token::DmdChain;

/// Hướng bridge token
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BridgeDirection {
    /// Từ EVM chain đến NEAR hub
    ToHub,
    /// Từ NEAR hub đến EVM chain
    FromHub,
}

/// Loại token trong quá trình bridge
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BridgeTokenType {
    /// Token ERC-1155 (standard DMD trên các chain EVM)
    Erc1155,
    /// Token ERC-20 (dùng trong quá trình bridge)
    Erc20,
    /// Token NEP-141 (standard DMD trên NEAR)
    Nep141,
    /// Token SPL (standard DMD trên Solana)
    Spl,
}

/// Trạng thái của giao dịch bridge
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BridgeStatus {
    /// Đã khởi tạo giao dịch bridge
    Initiated,
    /// Đang wrap/unwrap token
    TokenProcessing,
    /// Đang chờ LayerZero relayer
    AwaitingRelayer,
    /// Đang gửi token qua bridge
    Sending,
    /// Đang chờ xác nhận giao dịch
    AwaitingConfirmation,
    /// Đang xử lý trên chain đích
    Processing,
    /// Giao dịch hoàn tất thành công
    Completed,
    /// Giao dịch thất bại
    Failed,
    /// Giao dịch đã hết hạn
    Expired,
}

/// Chi tiết giao dịch bridge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeTransaction {
    /// ID giao dịch bridge
    pub id: String,
    /// Chain nguồn
    pub from_chain: DmdChain,
    /// Chain đích
    pub to_chain: DmdChain,
    /// Địa chỉ người gửi
    pub from_address: String,
    /// Địa chỉ người nhận
    pub to_address: String,
    /// Số lượng token
    pub amount: U256,
    /// Phí bridge
    pub fee: U256,
    /// Loại token trên chain nguồn
    pub source_token_type: BridgeTokenType,
    /// Loại token trên chain đích
    pub target_token_type: BridgeTokenType,
    /// Trạng thái hiện tại
    pub status: BridgeStatus,
    /// Hash giao dịch trên chain nguồn
    pub source_tx_hash: Option<String>,
    /// Hash giao dịch trên chain đích
    pub target_tx_hash: Option<String>,
    /// Thời gian bắt đầu
    pub initiated_at: DateTime<Utc>,
    /// Thời gian cập nhật cuối
    pub updated_at: DateTime<Utc>,
    /// Thời gian hoàn tất (nếu đã hoàn tất)
    pub completed_at: Option<DateTime<Utc>>,
    /// Thông tin lỗi (nếu có)
    pub error_message: Option<String>,
}

/// Cấu hình cho bridge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeConfig {
    /// ID của token DMD trên chain EVM (ERC-1155 token ID)
    pub dmd_token_id: u32,
    /// Địa chỉ contract ERC-1155 trên BSC
    pub bsc_erc1155_address: String,
    /// Địa chỉ contract ERC-20 trên BSC
    pub bsc_erc20_bridge_address: String,
    /// LayerZero endpoint trên BSC
    pub bsc_lz_endpoint: String,
    /// Địa chỉ contract ERC-1155 trên Ethereum
    pub eth_erc1155_address: String,
    /// Địa chỉ contract ERC-20 trên Ethereum
    pub eth_erc20_bridge_address: String,
    /// LayerZero endpoint trên Ethereum
    pub eth_lz_endpoint: String,
    /// ID của contract bridge trên NEAR
    pub near_bridge_contract_id: String,
    /// ID của contract token DMD trên NEAR
    pub near_token_contract_id: String,
    /// Phí cơ bản cho việc bridge (đơn vị: gas units)
    pub base_fee_gas: u64,
    /// Thời gian tối đa chờ xác nhận (giây)
    pub confirmation_timeout: u64,
    /// Số xác nhận tối thiểu trên EVM chains
    pub min_confirmations: u64,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            dmd_token_id: 1,
            bsc_erc1155_address: "0x16a7FA783c2FF584B234e13825960F95244Fc7bC".to_string(),
            bsc_erc20_bridge_address: "0x7B2c5b190C85b4a21A0F4A94Cff883bBc0594444".to_string(),
            bsc_lz_endpoint: "0x3c2269811836af69497E5F486A85D7316753cf62".to_string(),
            eth_erc1155_address: "0x90fE084F877C65e1b577c7b2eA64B8D8dd1AB278".to_string(),
            eth_erc20_bridge_address: "0x4A1f9D5182A3bA8218C30Ca30Eb9cf99F50e46eA".to_string(),
            eth_lz_endpoint: "0x66A71Dcef29A0fFBDBE3c6a460a3B5BC225Cd675".to_string(),
            near_bridge_contract_id: "dmd_bridge.near".to_string(),
            near_token_contract_id: "dmd.near".to_string(),
            base_fee_gas: 500000,
            confirmation_timeout: 600, // 10 phút
            min_confirmations: 12,
        }
    }
} 