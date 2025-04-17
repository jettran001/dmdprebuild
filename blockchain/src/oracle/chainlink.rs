//! Module triển khai nguồn dữ liệu Chainlink cho Oracle

use uuid::Uuid;
use chrono::Utc;
use async_trait::async_trait;
use tracing::{debug, warn};

use crate::smartcontracts::dmd_token::DmdChain;
use super::error::{OracleError, OracleResult};
use super::types::{OracleData, OracleDataType};
use super::provider::OracleDataSource;

/// Nguồn dữ liệu từ Chainlink
pub struct ChainlinkDataSource {
    /// API Endpoint
    api_endpoint: String,
    /// API Key
    api_key: Option<String>,
    /// Các chain được hỗ trợ
    supported_chains: Vec<DmdChain>,
}

#[async_trait]
impl OracleDataSource for ChainlinkDataSource {
    fn name(&self) -> &str {
        "Chainlink"
    }
    
    async fn fetch_data(&self, data_type: OracleDataType, chain: DmdChain) -> OracleResult<OracleData> {
        debug!("Lấy dữ liệu {} từ Chainlink cho chain {:?}", 
               format!("{:?}", data_type).to_lowercase(), chain);
        
        // Trong thực tế, sẽ gọi API Chainlink để lấy dữ liệu
        // Ở đây giả lập dữ liệu trả về
        
        let value = match data_type {
            OracleDataType::TokenPrice => {
                match chain {
                    DmdChain::Ethereum => "1.25",
                    DmdChain::BinanceSmartChain => "1.24",
                    DmdChain::Polygon => "1.23",
                    DmdChain::Avalanche => "1.22",
                    DmdChain::Near => "1.21",
                    _ => "1.20",
                }
            },
            OracleDataType::TotalSupply => "1000000000000000000000000", // 1 triệu token
            OracleDataType::GasFee => {
                match chain {
                    DmdChain::Ethereum => "50",
                    DmdChain::BinanceSmartChain => "5",
                    DmdChain::Polygon => "10",
                    DmdChain::Avalanche => "25",
                    _ => "10",
                }
            },
            _ => "",
        };
        
        if value.is_empty() {
            warn!("Chainlink không hỗ trợ dữ liệu {:?} cho chain {:?}", data_type, chain);
            return Err(OracleError::DataNotFound(format!(
                "Chainlink không hỗ trợ dữ liệu {:?} cho chain {:?}",
                data_type, chain
            )));
        }
        
        debug!("Lấy dữ liệu từ Chainlink thành công: {}", value);
        
        Ok(OracleData {
            id: Uuid::new_v4().to_string(),
            data_type,
            chain,
            value: value.to_string(),
            timestamp: Utc::now(),
            source: self.name().to_string(),
            signature: None,
        })
    }
    
    fn supports(&self, data_type: &OracleDataType, chain: &DmdChain) -> bool {
        if !self.supported_chains.contains(chain) {
            return false;
        }
        
        matches!(data_type, OracleDataType::TokenPrice | OracleDataType::TotalSupply | OracleDataType::GasFee)
    }
}

impl ChainlinkDataSource {
    /// Tạo nguồn dữ liệu Chainlink mới
    pub fn new(api_endpoint: &str, api_key: Option<String>) -> Self {
        Self {
            api_endpoint: api_endpoint.to_string(),
            api_key,
            supported_chains: vec![
                DmdChain::Ethereum,
                DmdChain::BinanceSmartChain,
                DmdChain::Polygon,
                DmdChain::Avalanche,
                DmdChain::Optimism,
                DmdChain::Arbitrum,
            ],
        }
    }
    
    /// Thêm chain được hỗ trợ
    pub fn add_supported_chain(&mut self, chain: DmdChain) -> &mut Self {
        if !self.supported_chains.contains(&chain) {
            self.supported_chains.push(chain);
        }
        self
    }
    
    /// Xóa chain khỏi danh sách hỗ trợ
    pub fn remove_supported_chain(&mut self, chain: &DmdChain) -> &mut Self {
        self.supported_chains.retain(|c| c != chain);
        self
    }
} 