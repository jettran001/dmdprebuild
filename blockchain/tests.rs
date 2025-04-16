//! Tests cho module blockchain

#[cfg(test)]
mod bsc_contract_tests {
    use crate::smartcontracts::bsc_contract::*;
    use anyhow::Result;
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;
    use ethers::providers::{Http, Provider};

    /// Test config mặc định
    #[test]
    fn test_default_config() {
        let config = BscContractConfig::default();
        assert_eq!(config.rpc_url, "https://bsc-dataseed.binance.org");
        assert_eq!(config.contract_address, "0x16a7FA783c2FF584B234e13825960F95244Fc7bC");
    }

    /// Test DmdBscContract mặc định
    #[test]
    fn test_dmd_bsc_contract() {
        let contract = DmdBscContract::default();
        assert_eq!(contract.contract_address, "0x16a7FA783c2FF584B234e13825960F95244Fc7bC");
        assert_eq!(contract.lz_endpoint, "0x3c2269811836af69497E5F486A85D7316753cf62");
    }

    /// Test đọc file Solidity
    #[tokio::test]
    async fn test_solidity_file_exists() -> Result<()> {
        // Kiểm tra file Solidity tồn tại
        let path = Path::new(SOLIDITY_FILE_PATH);
        assert!(path.exists(), "File Solidity phải tồn tại");
        
        // Đọc nội dung để kiểm tra
        let content = fs::read_to_string(path)?;
        assert!(!content.is_empty(), "Nội dung file Solidity không được rỗng");
        assert!(content.contains("contract DiamondToken"), "File không chứa contract DiamondToken");
        
        Ok(())
    }

    /// Test token interface
    #[test]
    fn test_token_interface() {
        let provider = BscContractProvider {
            config: BscContractConfig::default(),
            provider: Arc::new(Provider::<Http>::try_from("https://bsc-dataseed.binance.org").unwrap()),
        };
        
        assert_eq!(provider.decimals(), 18);
        assert_eq!(provider.token_name(), "Diamond Token");
        assert_eq!(provider.token_symbol(), "DMD");
    }
} 