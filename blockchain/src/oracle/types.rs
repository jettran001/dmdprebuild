//! Module định nghĩa các kiểu dữ liệu cho Oracle

use chrono::{DateTime, Utc};
use std::collections::HashMap;
use ethers::types::U256;

use crate::smartcontracts::dmd_token::DmdChain;

/// Loại dữ liệu Oracle cung cấp
#[derive(Debug, Clone, PartialEq)]
pub enum OracleDataType {
    /// Tỷ giá token (so với USD)
    TokenPrice,
    /// Tổng cung của token
    TotalSupply,
    /// Xác thực giao dịch cross-chain
    CrossChainVerification,
    /// Thông tin về phí gas
    GasFee,
    /// Thông tin về lượng token bị khóa trong bridge
    LockedTokens,
    /// Cảnh báo bất thường
    AnomalyAlert,
}

/// Dữ liệu Oracle
#[derive(Debug, Clone)]
pub struct OracleData {
    /// ID của dữ liệu
    pub id: String,
    /// Loại dữ liệu
    pub data_type: OracleDataType,
    /// Blockchain liên quan
    pub chain: DmdChain,
    /// Giá trị dữ liệu dưới dạng chuỗi (có thể chuyển đổi theo định dạng cần thiết)
    pub value: String,
    /// Thời gian tạo dữ liệu
    pub timestamp: DateTime<Utc>,
    /// Nguồn dữ liệu (API, contract, v.v.)
    pub source: String,
    /// Chữ ký xác thực (nếu có)
    pub signature: Option<String>,
}

/// Trạng thái báo cáo Oracle
#[derive(Debug, Clone, PartialEq)]
pub enum OracleReportStatus {
    /// Đang chờ xác nhận
    Pending,
    /// Đã được xác nhận
    Confirmed,
    /// Đã bị từ chối
    Rejected,
    /// Có tranh chấp
    Disputed,
}

/// Báo cáo Oracle
#[derive(Debug, Clone)]
pub struct OracleReport {
    /// ID báo cáo
    pub id: String,
    /// Dữ liệu trong báo cáo
    pub data: Vec<OracleData>,
    /// Người tạo báo cáo
    pub reporter: String,
    /// Thời gian tạo báo cáo
    pub created_at: DateTime<Utc>,
    /// Thời gian cập nhật báo cáo
    pub updated_at: DateTime<Utc>,
    /// Trạng thái báo cáo
    pub status: OracleReportStatus,
}

/// Cấu hình Oracle
#[derive(Debug, Clone)]
pub struct OracleConfig {
    /// Thời gian hết hạn cache (giây)
    pub cache_ttl: u64,
    /// Danh sách các chain được hỗ trợ
    pub supported_chains: Vec<DmdChain>,
    /// Cấu hình tùy chỉnh cho từng chain
    pub chain_configs: HashMap<DmdChain, ChainConfig>,
    /// Các API endpoints
    pub api_endpoints: HashMap<String, String>,
    /// Các API keys
    pub api_keys: HashMap<String, String>,
    /// Số lần thử lại tối đa khi gặp lỗi
    pub max_retries: u32,
    /// Thời gian chờ tối đa cho một request (mili giây)
    pub timeout_ms: u64,
}

/// Cấu hình cho một blockchain cụ thể
#[derive(Debug, Clone)]
pub struct ChainConfig {
    /// RPC URL
    pub rpc_url: String,
    /// Chain ID
    pub chain_id: u64,
    /// Địa chỉ contract oracle
    pub oracle_address: Option<String>,
    /// Địa chỉ contract bridge
    pub bridge_address: Option<String>,
    /// Địa chỉ contract token
    pub token_address: Option<String>,
}

impl Default for OracleConfig {
    fn default() -> Self {
        let mut supported_chains = Vec::new();
        supported_chains.push(DmdChain::Ethereum);
        supported_chains.push(DmdChain::BinanceSmartChain);
        supported_chains.push(DmdChain::Polygon);
        supported_chains.push(DmdChain::Avalanche);
        supported_chains.push(DmdChain::Near);
        
        let mut chain_configs = HashMap::new();
        chain_configs.insert(DmdChain::Ethereum, ChainConfig {
            rpc_url: "https://mainnet.infura.io/v3/your-api-key".to_string(),
            chain_id: 1,
            oracle_address: Some("0x1234...".to_string()),
            bridge_address: Some("0x2345...".to_string()),
            token_address: Some("0x3456...".to_string()),
        });
        
        chain_configs.insert(DmdChain::BinanceSmartChain, ChainConfig {
            rpc_url: "https://bsc-dataseed.binance.org/".to_string(),
            chain_id: 56,
            oracle_address: Some("0x2345...".to_string()),
            bridge_address: Some("0x3456...".to_string()),
            token_address: Some("0x4567...".to_string()),
        });
        
        let mut api_endpoints = HashMap::new();
        api_endpoints.insert("chainlink".to_string(), "https://api.chainlink.com".to_string());
        
        let mut api_keys = HashMap::new();
        api_keys.insert("chainlink".to_string(), "your-api-key".to_string());
        
        Self {
            cache_ttl: 300, // 5 phút
            supported_chains,
            chain_configs,
            api_endpoints,
            api_keys,
            max_retries: 3,
            timeout_ms: 30000, // 30 giây
        }
    }
}

/// Thông tin tổng hợp về token trên nhiều chain
#[derive(Debug, Clone)]
pub struct TokenDistribution {
    /// Tổng cung trên mỗi chain
    pub total_supplies: HashMap<DmdChain, U256>,
    /// Số lượng token bị khóa trên mỗi chain
    pub locked_tokens: HashMap<DmdChain, U256>,
    /// Giá trên mỗi chain
    pub prices: HashMap<DmdChain, f64>,
    /// Tổng giá trị của token trên mỗi chain (USD)
    pub total_values: HashMap<DmdChain, f64>,
    /// Thời gian cập nhật
    pub updated_at: DateTime<Utc>,
} 