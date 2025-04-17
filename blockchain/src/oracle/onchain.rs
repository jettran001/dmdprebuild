//! Module triển khai nguồn dữ liệu on-chain cho Oracle

use std::collections::HashMap;
use uuid::Uuid;
use chrono::Utc;
use async_trait::async_trait;
use tracing::{debug, warn};

use crate::smartcontracts::dmd_token::DmdChain;
use super::error::{OracleError, OracleResult};
use super::types::{OracleData, OracleDataType};
use super::provider::OracleDataSource;

/// Nguồn dữ liệu từ Contract On-chain
pub struct OnChainDataSource {
    /// Các RPC endpoint cho từng chain
    rpc_endpoints: HashMap<DmdChain, String>,
    /// Các địa chỉ contract data feed
    contract_addresses: HashMap<DmdChain, String>,
}

#[async_trait]
impl OracleDataSource for OnChainDataSource {
    fn name(&self) -> &str {
        "OnChain"
    }
    
    async fn fetch_data(&self, data_type: OracleDataType, chain: DmdChain) -> OracleResult<OracleData> {
        debug!("Lấy dữ liệu {} từ OnChain cho chain {:?}", 
               format!("{:?}", data_type).to_lowercase(), chain);
        
        // Kiểm tra xem có RPC endpoint và contract address cho chain này không
        if !self.rpc_endpoints.contains_key(&chain) || !self.contract_addresses.contains_key(&chain) {
            warn!("Không có endpoint hoặc contract address cho chain {:?}", chain);
            return Err(OracleError::DataNotFound(format!(
                "Không có endpoint hoặc contract address cho chain {:?}",
                chain
            )));
        }
        
        // Trong thực tế, sẽ gọi đến contract trên chain để lấy dữ liệu
        // Ở đây giả lập dữ liệu trả về
        
        let value = match data_type {
            OracleDataType::LockedTokens => {
                match chain {
                    DmdChain::Ethereum => "500000000000000000000", // 500 token
                    DmdChain::BinanceSmartChain => "300000000000000000000", // 300 token
                    DmdChain::Polygon => "200000000000000000000", // 200 token
                    DmdChain::Avalanche => "100000000000000000000", // 100 token
                    _ => "50000000000000000000", // 50 token
                }
            },
            OracleDataType::CrossChainVerification => "verified", // Giả lập kết quả xác minh
            _ => "",
        };
        
        if value.is_empty() {
            warn!("OnChain không hỗ trợ dữ liệu {:?} cho chain {:?}", data_type, chain);
            return Err(OracleError::DataNotFound(format!(
                "OnChain không hỗ trợ dữ liệu {:?} cho chain {:?}",
                data_type, chain
            )));
        }
        
        debug!("Lấy dữ liệu từ OnChain thành công: {}", value);
        
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
        if !self.rpc_endpoints.contains_key(chain) || !self.contract_addresses.contains_key(chain) {
            return false;
        }
        
        matches!(data_type, OracleDataType::LockedTokens | OracleDataType::CrossChainVerification)
    }
}

impl OnChainDataSource {
    /// Tạo nguồn dữ liệu on-chain mới
    pub fn new() -> Self {
        let mut rpc_endpoints = HashMap::new();
        rpc_endpoints.insert(DmdChain::Ethereum, "https://mainnet.infura.io/v3/your-api-key".to_string());
        rpc_endpoints.insert(DmdChain::BinanceSmartChain, "https://bsc-dataseed.binance.org/".to_string());
        rpc_endpoints.insert(DmdChain::Polygon, "https://polygon-rpc.com".to_string());
        rpc_endpoints.insert(DmdChain::Avalanche, "https://api.avax.network/ext/bc/C/rpc".to_string());
        
        let mut contract_addresses = HashMap::new();
        contract_addresses.insert(DmdChain::Ethereum, "0x1234...".to_string());
        contract_addresses.insert(DmdChain::BinanceSmartChain, "0x2345...".to_string());
        contract_addresses.insert(DmdChain::Polygon, "0x3456...".to_string());
        contract_addresses.insert(DmdChain::Avalanche, "0x4567...".to_string());
        
        Self {
            rpc_endpoints,
            contract_addresses,
        }
    }
    
    /// Thêm chain mới
    pub fn add_chain(&mut self, chain: DmdChain, rpc_endpoint: &str, contract_address: &str) -> &mut Self {
        self.rpc_endpoints.insert(chain, rpc_endpoint.to_string());
        self.contract_addresses.insert(chain, contract_address.to_string());
        self
    }
    
    /// Xóa chain
    pub fn remove_chain(&mut self, chain: &DmdChain) -> &mut Self {
        self.rpc_endpoints.remove(chain);
        self.contract_addresses.remove(chain);
        self
    }
    
    /// Cập nhật RPC endpoint cho chain
    pub fn update_rpc_endpoint(&mut self, chain: DmdChain, rpc_endpoint: &str) -> &mut Self {
        self.rpc_endpoints.insert(chain, rpc_endpoint.to_string());
        self
    }
    
    /// Cập nhật địa chỉ contract cho chain
    pub fn update_contract_address(&mut self, chain: DmdChain, contract_address: &str) -> &mut Self {
        self.contract_addresses.insert(chain, contract_address.to_string());
        self
    }
} 