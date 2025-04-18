//! Module định nghĩa các kiểu dữ liệu cho Oracle

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use ethers::types::U256;
use log::info;
use std::sync::Arc;
use std::collections::HashSet;

use crate::smartcontracts::dmd_token::DmdChain;
use crate::common::chain_types::DmdChain;

/// Thông tin token để định danh trong Oracle
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TokenIdentifier {
    /// Địa chỉ contract của token
    pub address: String,
    /// Chain mà token được định nghĩa trên
    pub chain: DmdChain,
    /// Symbol của token (ví dụ: ETH, USDT)
    pub symbol: Option<String>,
}

impl TokenIdentifier {
    /// Tạo token identifier mới
    pub fn new(address: String, chain: DmdChain, symbol: Option<String>) -> Self {
        Self {
            address,
            chain,
            symbol,
        }
    }
    
    /// Tạo identifier theo key format
    pub fn to_key(&self) -> String {
        format!("{}:{}", self.chain.name(), self.address)
    }
    
    /// Tạo từ key format
    pub fn from_key(key: &str) -> Option<Self> {
        let parts: Vec<&str> = key.split(':').collect();
        if parts.len() != 2 {
            return None;
        }
        
        let chain = DmdChain::from_str(parts[0]).ok()?;
        let address = parts[1].to_string();
        
        Some(Self {
            address,
            chain,
            symbol: None,
        })
    }
}

/// Loại dữ liệu Oracle cung cấp
#[derive(Debug, Clone, PartialEq)]
pub enum OracleDataType {
    /// Tỷ giá token (so với USD)
    TokenPrice(TokenIdentifier),
    /// Tỷ giá giữa hai token
    TokenPricePair {
        base_token: TokenIdentifier,
        quote_token: TokenIdentifier,
    },
    /// Tổng cung của token
    TotalSupply(TokenIdentifier),
    /// Xác thực giao dịch cross-chain
    CrossChainVerification {
        token: TokenIdentifier,
        source_chain: DmdChain,
        target_chain: DmdChain,
        tx_hash: String,
    },
    /// Thông tin về phí gas
    GasFee(DmdChain),
    /// Thông tin về lượng token bị khóa trong bridge
    LockedTokens(TokenIdentifier),
    /// Cảnh báo bất thường
    AnomalyAlert {
        token: Option<TokenIdentifier>,
        alert_type: String,
        severity: u8,
    },
}

impl OracleDataType {
    /// Lấy tên loại dữ liệu để hiển thị/log
    pub fn type_name(&self) -> &'static str {
        match self {
            OracleDataType::TokenPrice(_) => "TokenPrice",
            OracleDataType::TokenPricePair { .. } => "TokenPricePair",
            OracleDataType::TotalSupply(_) => "TotalSupply",
            OracleDataType::CrossChainVerification { .. } => "CrossChainVerification",
            OracleDataType::GasFee(_) => "GasFee",
            OracleDataType::LockedTokens(_) => "LockedTokens",
            OracleDataType::AnomalyAlert { .. } => "AnomalyAlert",
        }
    }
    
    /// Lấy chain liên quan đến loại dữ liệu
    pub fn related_chain(&self) -> Option<DmdChain> {
        match self {
            OracleDataType::TokenPrice(token) => Some(token.chain),
            OracleDataType::TokenPricePair { base_token, .. } => Some(base_token.chain),
            OracleDataType::TotalSupply(token) => Some(token.chain),
            OracleDataType::CrossChainVerification { source_chain, .. } => Some(*source_chain),
            OracleDataType::GasFee(chain) => Some(*chain),
            OracleDataType::LockedTokens(token) => Some(token.chain),
            OracleDataType::AnomalyAlert { token, .. } => token.as_ref().map(|t| t.chain),
        }
    }
    
    /// Lấy token liên quan đến loại dữ liệu nếu có
    pub fn related_token(&self) -> Option<&TokenIdentifier> {
        match self {
            OracleDataType::TokenPrice(token) => Some(token),
            OracleDataType::TokenPricePair { base_token, .. } => Some(base_token),
            OracleDataType::TotalSupply(token) => Some(token),
            OracleDataType::CrossChainVerification { token, .. } => Some(token),
            OracleDataType::LockedTokens(token) => Some(token),
            OracleDataType::AnomalyAlert { token, .. } => token.as_ref(),
            _ => None,
        }
    }
    
    /// Chuyển đổi sang định dạng key để sử dụng trong map
    pub fn to_key(&self) -> String {
        match self {
            OracleDataType::TokenPrice(token) => format!("price:{}", token.to_key()),
            OracleDataType::TokenPricePair { base_token, quote_token } => 
                format!("price_pair:{}:{}", base_token.to_key(), quote_token.to_key()),
            OracleDataType::TotalSupply(token) => format!("supply:{}", token.to_key()),
            OracleDataType::CrossChainVerification { token, source_chain, target_chain, tx_hash } => 
                format!("verify:{}:{}:{}:{}", token.to_key(), source_chain, target_chain, tx_hash),
            OracleDataType::GasFee(chain) => format!("gas:{}", chain),
            OracleDataType::LockedTokens(token) => format!("locked:{}", token.to_key()),
            OracleDataType::AnomalyAlert { token, alert_type, severity } => {
                if let Some(t) = token {
                    format!("alert:{}:{}:{}", t.to_key(), alert_type, severity)
                } else {
                    format!("alert:global:{}:{}", alert_type, severity)
                }
            }
        }
    }
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

/// Danh sách các error pattern được coi là nghiêm trọng
#[derive(Debug, Clone, Default)]
pub struct CriticalErrorPatterns {
    /// Các mẫu error text được coi là nghiêm trọng
    patterns: HashSet<String>,
}

impl CriticalErrorPatterns {
    /// Tạo mới với các mẫu mặc định
    pub fn new() -> Self {
        let mut patterns = HashSet::new();
        // Thêm các mẫu mặc định
        patterns.insert("timeout".to_string());
        patterns.insert("connection refused".to_string());
        patterns.insert("authentication failed".to_string());
        patterns.insert("access denied".to_string());
        patterns.insert("rate limit exceeded".to_string());
        patterns.insert("insufficient funds".to_string());
        
        Self { patterns }
    }
    
    /// Thêm mẫu mới vào danh sách
    pub fn add_pattern(&mut self, pattern: String) {
        self.patterns.insert(pattern);
    }
    
    /// Xóa mẫu khỏi danh sách
    pub fn remove_pattern(&mut self, pattern: &str) {
        self.patterns.remove(pattern);
    }
    
    /// Kiểm tra xem thông báo lỗi có chứa mẫu nghiêm trọng không
    pub fn is_critical(&self, error_message: &str) -> bool {
        self.patterns.iter().any(|pattern| error_message.contains(pattern))
    }
    
    /// Lấy tất cả các mẫu
    pub fn get_patterns(&self) -> Vec<String> {
        self.patterns.iter().cloned().collect()
    }
}

/// Cấu hình phân tích lỗi
#[derive(Debug, Clone)]
pub struct ErrorAnalysisConfig {
    /// Các mẫu lỗi nghiêm trọng
    pub critical_patterns: Arc<CriticalErrorPatterns>,
    /// Số lượng lỗi tối thiểu để coi là có vấn đề
    pub min_errors_threshold: usize,
    /// Tỷ lệ lỗi nghiêm trọng để coi là có vấn đề
    pub critical_ratio_threshold: f64,
}

impl Default for ErrorAnalysisConfig {
    fn default() -> Self {
        Self {
            critical_patterns: Arc::new(CriticalErrorPatterns::new()),
            min_errors_threshold: 1,
            critical_ratio_threshold: 0.3, // 30%
        }
    }
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
    /// Chi tiết lỗi cho các dữ liệu không thể lấy được
    pub error_details: Option<HashMap<String, String>>,
}

impl OracleReport {
    /// Thêm chi tiết lỗi vào báo cáo
    pub fn add_error_details(&mut self, failed_data_types: Vec<OracleDataType>, error_details: HashMap<String, String>) {
        // Cập nhật thời gian cập nhật
        self.updated_at = Utc::now();
        
        // Lưu chi tiết lỗi
        self.error_details = Some(error_details);
        
        // Log thông tin
        if let Some(details) = &self.error_details {
            for (data_type, error) in details {
                info!("Báo cáo {}: Chi tiết lỗi cho {}: {}", self.id, data_type, error);
            }
        }
    }

    /// Phân tích lỗi từ báo cáo và trả về các lỗi nghiêm trọng
    /// sử dụng cấu hình lỗi tùy chỉnh
    pub fn analyze_errors_with_config(&self, config: &ErrorAnalysisConfig) -> Option<HashMap<String, String>> {
        if let Some(errors) = &self.error_details {
            // Lọc các lỗi nghiêm trọng theo cấu hình
            let critical_errors = errors.iter()
                .filter(|(_, error_msg)| {
                    config.critical_patterns.is_critical(error_msg)
                })
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<HashMap<String, String>>();
            
            // Kiểm tra ngưỡng số lượng lỗi tối thiểu
            if critical_errors.len() >= config.min_errors_threshold {
                // Kiểm tra tỷ lệ lỗi nghiêm trọng
                let critical_ratio = critical_errors.len() as f64 / errors.len() as f64;
                if critical_ratio >= config.critical_ratio_threshold {
                    info!("Phát hiện {} lỗi nghiêm trọng trong báo cáo {} (tỷ lệ: {:.2})", 
                          critical_errors.len(), self.id, critical_ratio);
                    return Some(critical_errors);
                }
            }
        }
        None
    }

    /// Phân tích lỗi từ báo cáo và trả về các lỗi nghiêm trọng
    /// sử dụng cấu hình mặc định
    pub fn analyze_errors(&self) -> Option<HashMap<String, String>> {
        self.analyze_errors_with_config(&ErrorAnalysisConfig::default())
    }
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

/// Thông tin token trên một blockchain cụ thể
#[derive(Debug, Clone)]
pub struct ChainTokenData {
    /// Chain mà dữ liệu này thuộc về
    pub chain: DmdChain,
    /// Tổng cung token trên chain
    pub total_supply: U256,
    /// Số lượng token bị khóa (ví dụ: trong bridge contracts)
    pub locked_amount: U256,
    /// Giá token (USD)
    pub price: f64,
    /// Tổng giá trị của token trên chain này (USD)
    pub total_value: f64,
    /// Thời gian cập nhật dữ liệu gần nhất
    pub updated_at: DateTime<Utc>,
}

impl ChainTokenData {
    /// Tạo mới với giá trị mặc định
    pub fn new(chain: DmdChain) -> Self {
        Self {
            chain,
            total_supply: U256::zero(),
            locked_amount: U256::zero(),
            price: 0.0,
            total_value: 0.0,
            updated_at: Utc::now(),
        }
    }
    
    /// Tạo mới với các giá trị cụ thể
    pub fn with_values(
        chain: DmdChain,
        total_supply: U256,
        locked_amount: U256,
        price: f64,
    ) -> Self {
        let total_value = if price > 0.0 {
            // Tính tổng giá trị dựa trên total_supply và price
            // Chuyển đổi U256 sang f64 để tính toán
            let supply_f64 = u128::try_from(total_supply)
                .unwrap_or(u128::MAX) as f64;
            supply_f64 * price
        } else {
            0.0
        };
        
        Self {
            chain,
            total_supply,
            locked_amount,
            price,
            total_value,
            updated_at: Utc::now(),
        }
    }
    
    /// Cập nhật giá và tính lại tổng giá trị
    pub fn update_price(&mut self, price: f64) {
        self.price = price;
        
        // Cập nhật tổng giá trị
        if price > 0.0 {
            let supply_f64 = u128::try_from(self.total_supply)
                .unwrap_or(u128::MAX) as f64;
            self.total_value = supply_f64 * price;
        }
        
        self.updated_at = Utc::now();
    }
    
    /// Cập nhật tổng cung và tính lại tổng giá trị
    pub fn update_total_supply(&mut self, total_supply: U256) {
        self.total_supply = total_supply;
        
        // Cập nhật tổng giá trị
        if self.price > 0.0 {
            let supply_f64 = u128::try_from(total_supply)
                .unwrap_or(u128::MAX) as f64;
            self.total_value = supply_f64 * self.price;
        }
        
        self.updated_at = Utc::now();
    }
    
    /// Cập nhật số lượng token bị khóa
    pub fn update_locked_amount(&mut self, locked_amount: U256) {
        self.locked_amount = locked_amount;
        self.updated_at = Utc::now();
    }
}

/// Thông tin tổng hợp về token trên nhiều chain
#[derive(Debug, Clone)]
pub struct TokenDistribution {
    /// Dữ liệu token cho từng chain
    pub chain_data: HashMap<DmdChain, ChainTokenData>,
    /// Thời gian cập nhật tổng thể
    pub updated_at: DateTime<Utc>,
}

impl TokenDistribution {
    /// Tạo mới với danh sách chain
    pub fn new(chains: Vec<DmdChain>) -> Self {
        let mut chain_data = HashMap::new();
        
        // Tạo dữ liệu mặc định cho mỗi chain
        for chain in chains {
            chain_data.insert(chain, ChainTokenData::new(chain));
        }
        
        Self {
            chain_data,
            updated_at: Utc::now(),
        }
    }
    
    /// Thêm hoặc cập nhật dữ liệu cho một chain
    pub fn update_chain_data(&mut self, data: ChainTokenData) {
        self.chain_data.insert(data.chain, data);
        self.updated_at = Utc::now();
    }
    
    /// Lấy tổng giá trị token trên tất cả các chain
    pub fn get_total_value(&self) -> f64 {
        self.chain_data.values()
            .map(|data| data.total_value)
            .sum()
    }
    
    /// Lấy tổng cung token trên tất cả các chain
    pub fn get_total_supply(&self) -> U256 {
        let mut total = U256::zero();
        for data in self.chain_data.values() {
            total = total.saturating_add(data.total_supply);
        }
        total
    }
    
    /// Lấy tổng token bị khóa
    pub fn get_total_locked(&self) -> U256 {
        let mut total = U256::zero();
        for data in self.chain_data.values() {
            total = total.saturating_add(data.locked_amount);
        }
        total
    }
    
    /// Chuyển đổi từ cấu trúc cũ sang mới
    #[deprecated(since = "0.3.0", note = "Sử dụng để chuyển đổi từ cấu trúc cũ")]
    pub fn from_legacy(
        total_supplies: HashMap<DmdChain, U256>,
        locked_tokens: HashMap<DmdChain, U256>,
        prices: HashMap<DmdChain, f64>,
        total_values: HashMap<DmdChain, f64>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        let mut chain_data = HashMap::new();
        
        // Tạo tập hợp tất cả các chain
        let mut all_chains = HashSet::new();
        for chain in total_supplies.keys() {
            all_chains.insert(*chain);
        }
        for chain in locked_tokens.keys() {
            all_chains.insert(*chain);
        }
        for chain in prices.keys() {
            all_chains.insert(*chain);
        }
        
        // Tạo dữ liệu cho mỗi chain
        for chain in all_chains {
            let total_supply = total_supplies.get(&chain).cloned().unwrap_or_default();
            let locked_amount = locked_tokens.get(&chain).cloned().unwrap_or_default();
            let price = prices.get(&chain).cloned().unwrap_or_default();
            let total_value = total_values.get(&chain).cloned().unwrap_or_else(|| {
                // Tính tổng giá trị nếu không có sẵn
                let supply_f64 = u128::try_from(total_supply).unwrap_or(u128::MAX) as f64;
                supply_f64 * price
            });
            
            let mut data = ChainTokenData::new(chain);
            data.total_supply = total_supply;
            data.locked_amount = locked_amount;
            data.price = price;
            data.total_value = total_value;
            data.updated_at = updated_at;
            
            chain_data.insert(chain, data);
        }
        
        Self {
            chain_data,
            updated_at,
        }
    }
    
    /// Cập nhật thông tin dựa trên báo cáo Oracle
    pub fn update_from_oracle_report(&mut self, report: &OracleReport) {
        for data in &report.data {
            match &data.data_type {
                OracleDataType::TokenPrice(token_id) => {
                    if let Some(chain_data) = self.chain_data.get_mut(&token_id.chain) {
                        // Chuyển đổi giá từ chuỗi sang f64
                        if let Ok(price) = data.value.parse::<f64>() {
                            chain_data.update_price(price);
                        }
                    }
                },
                OracleDataType::TotalSupply(token_id) => {
                    if let Some(chain_data) = self.chain_data.get_mut(&token_id.chain) {
                        // Chuyển đổi tổng cung từ chuỗi sang U256
                        if data.value.starts_with("0x") {
                            if let Ok(supply) = U256::from_str(&data.value) {
                                chain_data.update_total_supply(supply);
                            }
                        } else {
                            if let Ok(supply) = U256::from_dec_str(&data.value) {
                                chain_data.update_total_supply(supply);
                            }
                        }
                    }
                },
                OracleDataType::LockedTokens(token_id) => {
                    if let Some(chain_data) = self.chain_data.get_mut(&token_id.chain) {
                        // Chuyển đổi số lượng khóa từ chuỗi sang U256
                        if data.value.starts_with("0x") {
                            if let Ok(locked) = U256::from_str(&data.value) {
                                chain_data.update_locked_amount(locked);
                            }
                        } else {
                            if let Ok(locked) = U256::from_dec_str(&data.value) {
                                chain_data.update_locked_amount(locked);
                            }
                        }
                    }
                },
                _ => {} // Bỏ qua các loại dữ liệu khác
            }
        }
        
        self.updated_at = Utc::now();
    }
} 