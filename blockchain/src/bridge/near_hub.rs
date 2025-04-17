//! Logic bridge với NEAR làm hub

use std::str::FromStr;
use std::sync::Arc;
use std::collections::HashMap;
use anyhow::{Result, Context};
use async_trait::async_trait;
use uuid::Uuid;
use ethers::types::U256;
use chrono::Utc;
use tokio::sync::RwLock;
use tracing::{info, debug, warn, error};

use crate::smartcontracts::{
    dmd_token::DmdChain,
    near_contract::NearContractProvider,
};

use super::{
    error::{BridgeError, BridgeResult},
    traits::BridgeHub,
    types::{BridgeTransaction, BridgeStatus, BridgeConfig, BridgeTokenType},
};

/// ABI của NEAR bridge contract
const NEAR_BRIDGE_CONTRACT_ABI: &str = r#"
[
  {
    "methods": [
      {
        "name": "bridge_to_evm",
        "kind": "call",
        "params": {
          "serialization_type": "json",
          "args": [
            {
              "name": "evm_chain_id",
              "type": "u16",
              "description": "LayerZero chain ID cho EVM"
            },
            {
              "name": "recipient",
              "type": "string",
              "description": "Địa chỉ nhận trên chain EVM"
            },
            {
              "name": "amount",
              "type": "string",
              "description": "Số lượng token cần bridge"
            }
          ]
        },
        "deposit": "0.1",
        "gas": 50000000000000
      },
      {
        "name": "receive_from_evm",
        "kind": "call",
        "params": {
          "serialization_type": "json",
          "args": [
            {
              "name": "sender",
              "type": "string",
              "description": "Địa chỉ gửi từ chain EVM"
            },
            {
              "name": "near_account",
              "type": "string",
              "description": "Tài khoản NEAR nhận token"
            },
            {
              "name": "amount",
              "type": "string",
              "description": "Số lượng token"
            },
            {
              "name": "proof",
              "type": "string",
              "description": "Chứng minh giao dịch từ LayerZero"
            }
          ]
        },
        "deposit": "0",
        "gas": 50000000000000
      }
    ]
  }
]
"#;

/// Bridge hub sử dụng NEAR Protocol
pub struct NearBridgeHub {
    /// Cấu hình bridge
    config: BridgeConfig,
    /// Provider cho NEAR
    near_provider: Option<Arc<NearContractProvider>>,
    /// Cache các giao dịch bridge đang xử lý
    transactions: Arc<RwLock<HashMap<String, BridgeTransaction>>>,
    /// Cache các chain hỗ trợ
    supported_chains: Vec<DmdChain>,
    /// LayerZero chain ID mapping
    lz_chain_map: HashMap<DmdChain, u16>,
}

impl Default for NearBridgeHub {
    fn default() -> Self {
        let mut lz_chain_map = HashMap::new();
        lz_chain_map.insert(DmdChain::Ethereum, 101);
        lz_chain_map.insert(DmdChain::BinanceSmartChain, 102);
        lz_chain_map.insert(DmdChain::Avalanche, 106);
        lz_chain_map.insert(DmdChain::Polygon, 109);
        lz_chain_map.insert(DmdChain::Arbitrum, 110);
        lz_chain_map.insert(DmdChain::Optimism, 111);
        lz_chain_map.insert(DmdChain::Fantom, 112);
        lz_chain_map.insert(DmdChain::Near, 115);

        Self {
            config: BridgeConfig::default(),
            near_provider: None,
            transactions: Arc::new(RwLock::new(HashMap::new())),
            supported_chains: vec![
                DmdChain::Ethereum,
                DmdChain::BinanceSmartChain,
                DmdChain::Avalanche,
                DmdChain::Polygon,
                DmdChain::Arbitrum,
                DmdChain::Optimism,
                DmdChain::Base,
            ],
            lz_chain_map,
        }
    }
}

impl NearBridgeHub {
    /// Tạo hub bridge mới
    pub fn new() -> Self {
        Self::default()
    }

    /// Tạo hub bridge với cấu hình tùy chỉnh
    pub fn with_config(config: BridgeConfig) -> Self {
        Self {
            config,
            ..Self::default()
        }
    }
    
    /// Lấy LayerZero chain ID cho chain cụ thể
    fn get_lz_chain_id(&self, chain: DmdChain) -> BridgeResult<u16> {
        self.lz_chain_map
            .get(&chain)
            .copied()
            .ok_or_else(|| BridgeError::UnsupportedRoute(
                format!("Route không được hỗ trợ: NEAR -> {:?}", chain)
            ))
    }
    
    /// Tạo transaction bridge mới
    fn create_transaction(
        &self,
        from_chain: DmdChain,
        to_chain: DmdChain,
        from_address: &str,
        to_address: &str,
        amount: U256,
        source_token_type: BridgeTokenType,
        target_token_type: BridgeTokenType,
    ) -> BridgeTransaction {
        let tx_id = Uuid::new_v4().to_string();
        let now = Utc::now();
        
        BridgeTransaction {
            id: tx_id,
            from_chain,
            to_chain,
            from_address: from_address.to_string(),
            to_address: to_address.to_string(),
            amount,
            fee: U256::zero(), // Will be updated later
            source_token_type,
            target_token_type,
            status: BridgeStatus::Initiated,
            source_tx_hash: None,
            target_tx_hash: None,
            initiated_at: now,
            updated_at: now,
            completed_at: None,
            error_message: None,
        }
    }
    
    /// Lưu thông tin giao dịch mới
    async fn store_transaction(&self, tx: BridgeTransaction) -> BridgeResult<()> {
        let mut txs = self.transactions.write().await;
        txs.insert(tx.id.clone(), tx);
        Ok(())
    }
    
    /// Cập nhật thông tin giao dịch
    async fn update_transaction(
        &self,
        tx_id: &str,
        status: BridgeStatus,
        source_tx_hash: Option<String>,
        target_tx_hash: Option<String>,
        error_message: Option<String>,
    ) -> BridgeResult<BridgeTransaction> {
        let mut txs = self.transactions.write().await;
        
        if let Some(tx) = txs.get_mut(tx_id) {
            tx.status = status;
            tx.updated_at = Utc::now();
            
            if let Some(hash) = source_tx_hash {
                tx.source_tx_hash = Some(hash);
            }
            
            if let Some(hash) = target_tx_hash {
                tx.target_tx_hash = Some(hash);
            }
            
            if let Some(error) = error_message {
                tx.error_message = Some(error);
            }
            
            if status == BridgeStatus::Completed || status == BridgeStatus::Failed {
                tx.completed_at = Some(Utc::now());
            }
            
            Ok(tx.clone())
        } else {
            Err(BridgeError::TransactionNotFound(tx_id.to_string()))
        }
    }
    
    /// Kiểm tra NEAR provider đã được khởi tạo chưa
    fn ensure_provider_initialized(&self) -> BridgeResult<Arc<NearContractProvider>> {
        self.near_provider
            .clone()
            .ok_or_else(|| BridgeError::ProviderError("NEAR provider chưa được khởi tạo".to_string()))
    }
}

#[async_trait]
impl BridgeHub for NearBridgeHub {
    /// Khởi tạo hub với cấu hình cho trước
    async fn initialize(&mut self, config: BridgeConfig) -> BridgeResult<()> {
        self.config = config;
        
        // Khởi tạo NEAR provider
        let near_provider = NearContractProvider::new(None)
            .await
            .map_err(|e| BridgeError::ProviderError(format!("Không thể tạo NEAR provider: {}", e)))?;
        
        self.near_provider = Some(Arc::new(near_provider));
        
        info!("NEAR bridge hub đã khởi tạo với contract ID: {}", self.config.near_bridge_contract_id);
        Ok(())
    }
    
    /// Nhận token từ chain khác vào hub
    async fn receive_from_spoke(
        &self,
        from_chain: DmdChain,
        from_address: &str, 
        to_near_account: &str, 
        amount: U256
    ) -> BridgeResult<BridgeTransaction> {
        // Kiểm tra xem chain có được hỗ trợ không
        if !self.supported_chains.contains(&from_chain) {
            return Err(BridgeError::UnsupportedChain(
                format!("Chain không được hỗ trợ: {:?}", from_chain)
            ));
        }
        
        // Kiểm tra LayerZero chain ID
        if !self.lz_chain_map.contains_key(&from_chain) {
            return Err(BridgeError::UnsupportedChain(
                format!("Chain không có LZ chain ID: {:?}", from_chain)
            ));
        }
        
        // Kiểm tra địa chỉ nhận
        if to_near_account.is_empty() {
            return Err(BridgeError::InvalidAddress(
                "Địa chỉ NEAR account không được để trống".to_string()
            ));
        }
        
        // Kiểm tra số lượng
        if amount == U256::zero() {
            return Err(BridgeError::InvalidAmount(
                "Số lượng token phải lớn hơn 0".to_string()
            ));
        }
        
        // Kiểm tra provider
        let provider = self.ensure_provider_initialized()?;
        
        // Tạo giao dịch mới
        let tx = self.create_transaction(
            from_chain,
            DmdChain::Near,
            from_address,
            to_near_account,
            amount,
            BridgeTokenType::Erc20, // Từ ERC-20 trên EVM
            BridgeTokenType::Nep141, // Sang NEP-141 trên NEAR
        );
        
        // Lưu giao dịch
        self.store_transaction(tx.clone()).await?;
        
        // Mô phỏng giao dịch nhận
        let tx_hash = format!("0x{}", Uuid::new_v4().simple());
        
        // Cập nhật thông tin giao dịch
        let updated_tx = self.update_transaction(
            &tx.id,
            BridgeStatus::Processing,
            Some(tx_hash.clone()),
            None,
            None,
        ).await?;
        
        // Log thông tin
        info!(
            "Receiving token from {:?} to NEAR, from: {}, to: {}, amount: {}, tx_id: {}",
            from_chain, from_address, to_near_account, amount, tx.id
        );
        
        Ok(updated_tx)
    }
    
    /// Gửi token từ hub sang chain khác
    async fn send_to_spoke(
        &self,
        private_key: &str,
        to_chain: DmdChain,
        to_address: &str,
        amount: U256
    ) -> BridgeResult<BridgeTransaction> {
        let provider = self.ensure_provider_initialized()?;
        
        // Kiểm tra chain có được hỗ trợ không
        if !self.supported_chains.contains(&to_chain) {
            return Err(BridgeError::UnsupportedRoute(
                format!("Route không được hỗ trợ: NEAR -> {:?}", to_chain)
            ));
        }
        
        // Tạo giao dịch bridge mới
        let target_token_type = if super::error::is_evm_chain(&to_chain) {
            BridgeTokenType::Erc20 // Sẽ unwrap thành ERC-1155 sau
        } else {
            BridgeTokenType::Spl
        };
        
        // Lấy tài khoản NEAR từ private key
        let from_address = "near_sender.near".to_string(); // Thực tế sẽ lấy từ private key
        
        let tx = self.create_transaction(
            DmdChain::Near,
            to_chain,
            &from_address,
            to_address,
            amount,
            BridgeTokenType::Nep141,
            target_token_type,
        );
        
        // Cập nhật trạng thái giao dịch
        let lz_chain_id = self.get_lz_chain_id(to_chain)?;
        
        // Lưu thông tin giao dịch
        self.store_transaction(tx.clone()).await?;
        
        // Trong thực tế, sẽ gọi NEAR contract để bridge token qua LayerZero
        // Giả lập giao dịch
        let source_tx_hash = format!("near_tx_{}", Uuid::new_v4().to_string().split('-').next().unwrap());
        
        // Cập nhật trạng thái
        let updated_tx = self.update_transaction(
            &tx.id,
            BridgeStatus::Sending,
            Some(source_tx_hash.clone()),
            None,
            None,
        ).await?;
        
        info!(
            "Đã khởi tạo bridge từ NEAR -> {:?}, đến: {}, số lượng: {}, id: {}, tx: {}",
            to_chain, to_address, amount, tx.id, source_tx_hash
        );
        
        Ok(updated_tx)
    }
    
    /// Kiểm tra trạng thái giao dịch bridge
    async fn check_transaction_status(&self, tx_id: &str) -> BridgeResult<BridgeStatus> {
        let txs = self.transactions.read().await;
        
        if let Some(tx) = txs.get(tx_id) {
            Ok(tx.status)
        } else {
            Err(BridgeError::TransactionNotFound(tx_id.to_string()))
        }
    }
    
    /// Lấy thông tin giao dịch bridge
    async fn get_transaction(&self, tx_id: &str) -> BridgeResult<BridgeTransaction> {
        let txs = self.transactions.read().await;
        
        txs.get(tx_id)
            .cloned()
            .ok_or_else(|| BridgeError::TransactionNotFound(tx_id.to_string()))
    }
    
    /// Ước tính phí bridge
    async fn estimate_fee(&self, to_chain: DmdChain, amount: U256) -> BridgeResult<U256> {
        // Kiểm tra chain có được hỗ trợ không
        if !self.supported_chains.contains(&to_chain) {
            return Err(BridgeError::UnsupportedRoute(
                format!("Route không được hỗ trợ: NEAR -> {:?}", to_chain)
            ));
        }
        
        // Lấy LayerZero chain ID
        let lz_chain_id = self.get_lz_chain_id(to_chain)?;
        
        // Base fee
        let base_fee = U256::from(self.config.base_fee_gas);
        
        // Gas price ước tính (sẽ phụ thuộc vào chain đích)
        let gas_price = match to_chain {
            DmdChain::Ethereum => U256::from(50_000_000_000u64), // 50 gwei
            DmdChain::BinanceSmartChain => U256::from(5_000_000_000u64),       // 5 gwei
            DmdChain::Avalanche => U256::from(25_000_000_000u64), // 25 gwei
            DmdChain::Polygon => U256::from(100_000_000_000u64),  // 100 gwei
            DmdChain::Arbitrum => U256::from(1_000_000_000u64),   // 1 gwei
            DmdChain::Optimism => U256::from(1_000_000u64),       // 0.001 gwei
            _ => U256::from(10_000_000_000u64),                   // 10 gwei default
        };
        
        // LayerZero fee (ước tính)
        let lz_fee = U256::from(0.002 * 1e18 as u64); // ~0.002 ETH or equivalent
        
        // Tổng phí = gas_price * base_fee + lz_fee
        let total_fee = gas_price.checked_mul(base_fee)
            .ok_or_else(|| BridgeError::SystemError("Tràn số khi tính phí cơ bản".to_string()))?
            .checked_add(lz_fee)
            .ok_or_else(|| BridgeError::SystemError("Tràn số khi tính tổng phí".to_string()))?;
        
        Ok(total_fee)
    }
    
    /// Lấy danh sách các chain được hỗ trợ
    fn get_supported_chains(&self) -> Vec<DmdChain> {
        self.supported_chains.clone()
    }
    
    /// Dọn dẹp cache giao dịch
    async fn cleanup_transaction_cache(&self) -> BridgeResult<usize> {
        let mut txs = self.transactions.write().await;
        let initial_size = txs.len();
        
        // Thời điểm hiện tại
        let now = Utc::now();
        
        // Giữ lại các giao dịch trong vòng 24 giờ gần đây hoặc chưa hoàn thành
        let to_retain: HashMap<String, BridgeTransaction> = txs
            .drain()
            .filter(|(_, tx)| {
                // Giữ các giao dịch chưa hoàn thành
                if tx.status != BridgeStatus::Completed && tx.status != BridgeStatus::Failed {
                    return true;
                }
                
                // Hoặc giao dịch mới (trong vòng 24 giờ)
                let age = now.signed_duration_since(tx.initiated_at);
                age.num_hours() < 24
            })
            .collect();
        
        // Cập nhật lại cache
        *txs = to_retain;
        
        // Số lượng giao dịch đã xóa
        let removed = initial_size - txs.len();
        info!("Đã dọn dẹp {} giao dịch từ cache, còn lại {}", removed, txs.len());
        
        Ok(removed)
    }
    
    /// Quản lý cache tự động
    async fn manage_cache(&self) -> BridgeResult<()> {
        let MAX_CACHE_SIZE: usize = 5000; // Giới hạn cache size
        
        // Kiểm tra kích thước cache
        let cache_size = {
            let txs = self.transactions.read().await;
            txs.len()
        };
        
        // Nếu cache quá lớn, dọn dẹp
        if cache_size > MAX_CACHE_SIZE {
            debug!("Cache quá tải ({} giao dịch), đang dọn dẹp...", cache_size);
            self.cleanup_transaction_cache().await?;
        }
        
        Ok(())
    }
} 