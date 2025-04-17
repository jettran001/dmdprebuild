//! Module định nghĩa giao diện cho các nguồn dữ liệu Oracle

use async_trait::async_trait;
use crate::smartcontracts::dmd_token::DmdChain;
use super::error::OracleResult;
use super::types::{OracleData, OracleDataType};

/// Nguồn dữ liệu cho Oracle
#[async_trait]
pub trait OracleDataSource: Send + Sync + 'static {
    /// Tên của nguồn dữ liệu
    fn name(&self) -> &str;
    
    /// Lấy dữ liệu theo loại và chain cụ thể
    async fn fetch_data(&self, data_type: OracleDataType, chain: DmdChain) -> OracleResult<OracleData>;
    
    /// Kiểm tra xem nguồn dữ liệu có hỗ trợ loại dữ liệu và chain cho trước không
    fn supports(&self, data_type: &OracleDataType, chain: &DmdChain) -> bool;
} 