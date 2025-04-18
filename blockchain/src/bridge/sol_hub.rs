//! Logic bridge với Solana làm hub

use std::str::FromStr;
use std::sync::Arc;
use std::collections::HashMap;
use anyhow::{Result, Context};
use async_trait::async_trait;
use uuid::Uuid;
use ethers::types::U256;
use chrono::{Utc, DateTime};
use tokio::sync::RwLock;
use tracing::{info, debug, warn, error};
use rand;
use regex::Regex;
use std::time::{SystemTime, Duration};
use once_cell::sync::Lazy;

use crate::smartcontracts::{
    dmd_token::DmdChain,
    solana_program::SolanaProgramProvider,
};

use super::{
    error::{BridgeError, BridgeResult},
    traits::BridgeHub,
    types::{BridgeTransaction, BridgeStatus, BridgeConfig, BridgeTokenType},
    persistent_repository::{JsonBridgeTransactionRepository, BridgeTransactionRepository},
};

/// ABI của Solana bridge program
const SOLANA_BRIDGE_PROGRAM_ABI: &str = r#"
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
              "type": "u64",
              "description": "Số lượng token cần bridge"
            }
          ]
        },
        "fee": "0.01",
        "compute_units": 200000
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
              "name": "solana_account",
              "type": "string",
              "description": "Tài khoản Solana nhận token"
            },
            {
              "name": "amount",
              "type": "u64",
              "description": "Số lượng token"
            },
            {
              "name": "proof",
              "type": "string",
              "description": "Chứng minh giao dịch từ LayerZero"
            }
          ]
        },
        "fee": "0",
        "compute_units": 200000
      }
    ]
  }
]
"#;

/// Thêm các regex patterns dùng Lazy để thay thế các unwrap() trong hàm validate_address
/// Khai báo static patterns ở đầu module, ngoài các hàm
static EVM_ADDRESS_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^0x[0-9a-fA-F]{40}$")
        .expect("Không thể biên dịch regex pattern cho địa chỉ EVM")
});

static SOLANA_ADDRESS_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^[1-9A-HJ-NP-Za-km-z]{32,44}$")
        .expect("Không thể biên dịch regex pattern cho địa chỉ Solana")
});

/// Bridge hub sử dụng Solana Protocol
pub struct SolBridgeHub {
    /// Cấu hình bridge
    config: BridgeConfig,
    /// Provider cho Solana
    sol_provider: Option<Arc<SolanaProgramProvider>>,
    /// Cache các giao dịch bridge đang xử lý
    transactions: Arc<RwLock<HashMap<String, BridgeTransaction>>>,
    /// Cache các chain hỗ trợ
    supported_chains: Vec<DmdChain>,
    /// LayerZero chain ID mapping
    lz_chain_map: HashMap<DmdChain, u16>,
    /// Repository cho lưu trữ bền vững
    persistent_repo: Option<Arc<dyn BridgeTransactionRepository + Send + Sync>>,
}

impl Default for SolBridgeHub {
    fn default() -> Self {
        let mut lz_chain_map = HashMap::new();
        lz_chain_map.insert(DmdChain::Ethereum, 101);
        lz_chain_map.insert(DmdChain::BinanceSmartChain, 102);
        lz_chain_map.insert(DmdChain::Avalanche, 106);
        lz_chain_map.insert(DmdChain::Polygon, 109);
        lz_chain_map.insert(DmdChain::Arbitrum, 110);
        lz_chain_map.insert(DmdChain::Optimism, 111);
        lz_chain_map.insert(DmdChain::Fantom, 112);
        lz_chain_map.insert(DmdChain::Solana, 113);

        Self {
            config: BridgeConfig::default(),
            sol_provider: None,
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
            persistent_repo: None,
        }
    }
}

impl SolBridgeHub {
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
    
    /// Tạo hub bridge với repository lưu trữ bền vững
    pub fn with_persistent_repository(mut self, repo: Arc<dyn BridgeTransactionRepository + Send + Sync>) -> Self {
        self.persistent_repo = Some(repo);
        self
    }
    
    /// Khởi tạo repository lưu trữ JSON mặc định
    pub fn init_default_repository(&mut self, file_path: &str) -> BridgeResult<()> {
        match JsonBridgeTransactionRepository::new(file_path) {
            Ok(repo) => {
                let repo = repo.start_async_worker();
                self.persistent_repo = Some(Arc::new(repo));
                
                // Tải dữ liệu từ repository vào cache
                self.load_transactions_from_repository().await?;
                
                info!("Đã khởi tạo repository lưu trữ bền vững tại: {}", file_path);
                Ok(())
            },
            Err(e) => {
                Err(BridgeError::RepositoryError(format!(
                    "Không thể khởi tạo repository JSON: {}", e
                )))
            }
        }
    }
    
    /// Tải dữ liệu từ repository vào cache
    async fn load_transactions_from_repository(&self) -> BridgeResult<()> {
        if let Some(repo) = &self.persistent_repo {
            match repo.get_all_transactions() {
                Ok(transactions) => {
                    let mut tx_cache = self.transactions.write().await;
                    
                    for tx in transactions {
                        tx_cache.insert(tx.id.clone(), tx);
                    }
                    
                    info!("Đã tải {} giao dịch từ repository vào cache", tx_cache.len());
                    Ok(())
                },
                Err(e) => {
                    Err(BridgeError::RepositoryError(format!(
                        "Không thể tải giao dịch từ repository: {}", e
                    )))
                }
            }
        } else {
            // Không có repository, không cần tải
            Ok(())
        }
    }
    
    /// Lấy LayerZero chain ID cho chain cụ thể
    fn get_lz_chain_id(&self, chain: DmdChain) -> BridgeResult<u16> {
        self.lz_chain_map
            .get(&chain)
            .copied()
            .ok_or_else(|| BridgeError::UnsupportedRoute(
                format!("Route không được hỗ trợ: Solana -> {:?}", chain)
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
            fee: U256::zero(), // Sẽ được cập nhật sau
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
        // Lưu vào cache
        let mut txs = self.transactions.write().await;
        txs.insert(tx.id.clone(), tx.clone());
        
        // Lưu vào repository nếu có
        if let Some(repo) = &self.persistent_repo {
            if let Err(e) = repo.save(&tx) {
                warn!("Không thể lưu giao dịch vào repository: {}. Chỉ lưu trữ trong bộ nhớ.", e);
            } else {
                debug!("Đã lưu giao dịch {} vào repository", tx.id);
            }
        }
        
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
            
            let updated_tx = tx.clone();
            
            // Lưu thay đổi vào repository nếu có
            if let Some(repo) = &self.persistent_repo {
                if let Err(e) = repo.save(&updated_tx) {
                    warn!("Không thể cập nhật giao dịch trong repository: {}. Chỉ cập nhật trong bộ nhớ.", e);
                } else {
                    debug!("Đã cập nhật giao dịch {} trong repository", tx_id);
                }
            }
            
            Ok(updated_tx)
        } else {
            Err(BridgeError::TransactionNotFound(tx_id.to_string()))
        }
    }
    
    /// Kiểm tra Solana provider đã được khởi tạo chưa
    fn ensure_provider_initialized(&self) -> BridgeResult<Arc<SolanaProgramProvider>> {
        self.sol_provider
            .clone()
            .ok_or_else(|| BridgeError::ProviderError("Solana provider chưa được khởi tạo".to_string()))
    }

    /// Chờ giao dịch Solana được xác nhận với cơ chế timeout
    async fn wait_for_transaction_finality(&self, tx_hash: &str, timeout_seconds: u64) -> BridgeResult<bool> {
        let provider = self.ensure_provider_initialized()?;

        // Kiểm tra tính hợp lệ của hash giao dịch
        if tx_hash.is_empty() {
            return Err(BridgeError::InvalidTransactionHash(
                "Hash giao dịch không được để trống".to_string()
            ));
        }

        // Khởi tạo các tham số cho việc kiểm tra xác nhận
        let max_attempts = 60; // Số lần kiểm tra tối đa
        let initial_backoff_ms = 500; // Thời gian chờ ban đầu giữa các lần kiểm tra
        let max_backoff_ms = 5000; // Thời gian chờ tối đa giữa các lần kiểm tra
        let tx_hash_clone = tx_hash.to_string();
        
        // Sử dụng tokio channel để giao tiếp giữa các task
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        
        // Tạo task riêng để kiểm tra giao dịch
        let confirmation_task = tokio::spawn(async move {
            let mut backoff_ms = initial_backoff_ms;
            
            for attempt in 1..=max_attempts {
                debug!("Kiểm tra xác nhận giao dịch Solana lần {}/{}: {}", attempt, max_attempts, tx_hash_clone);
                
                // Giả lập kiểm tra xác nhận giao dịch (thực tế sẽ gọi Solana RPC)
                // TODO: Thay thế bằng lệnh gọi thực tế đến Solana RPC
                let is_finalized = match attempt {
                    // Giả lập: xác nhận giao dịch sau 3 lần thử
                    n if n >= 3 => true,
                    _ => false
                };
                
                if is_finalized {
                    // Gửi kết quả xác nhận
                    if tx.send(true).await.is_err() {
                        warn!("Không thể gửi kết quả xác nhận cho giao dịch {}", tx_hash_clone);
                    }
                    return;
                }
                
                // Tăng thời gian chờ theo exponential backoff
                tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
                backoff_ms = std::cmp::min(backoff_ms * 2, max_backoff_ms);
            }
            
            // Nếu đã hết số lần thử mà không thành công
            if tx.send(false).await.is_err() {
                warn!("Không thể gửi kết quả thất bại cho giao dịch {}", tx_hash_clone);
            }
        });
        
        // Thiết lập timeout
        let timeout_duration = std::time::Duration::from_secs(timeout_seconds);
        let timeout_future = tokio::time::sleep(timeout_duration);
        
        // Sử dụng tokio::select để xử lý kết quả hoặc timeout
        tokio::select! {
            // Nếu nhận được kết quả từ task kiểm tra
            result = rx.recv() => {
                match result {
                    Some(true) => {
                        info!("Giao dịch Solana đã được xác nhận: {}", tx_hash);
                        Ok(true)
                    },
                    Some(false) => {
                        warn!("Giao dịch Solana không được xác nhận sau {} lần thử: {}", max_attempts, tx_hash);
                        Err(BridgeError::TransactionFailed(format!("Giao dịch không được xác nhận: {}", tx_hash)))
                    },
                    None => {
                        warn!("Task kiểm tra xác nhận kết thúc không mong muốn: {}", tx_hash);
                        Err(BridgeError::SystemError(format!("Task kiểm tra xác nhận kết thúc không mong muốn: {}", tx_hash)))
                    }
                }
            },
            // Nếu timeout xảy ra trước
            _ = timeout_future => {
                // Hủy bỏ task kiểm tra
                confirmation_task.abort();
                error!("Đã vượt quá thời gian chờ {} giây khi chờ xác nhận giao dịch Solana: {}", timeout_seconds, tx_hash);
                Err(BridgeError::TimeoutError(format!("Đã vượt quá thời gian chờ {} giây: {}", timeout_seconds, tx_hash)))
            }
        }
    }

    /// Kiểm tra tính hợp lệ của địa chỉ dựa trên chain
    fn validate_address(&self, address: &str, chain: &DmdChain) -> BridgeResult<()> {
        if address.is_empty() {
            return Err(BridgeError::InvalidAddress("Địa chỉ không được để trống".to_string()));
        }
        
        match chain {
            DmdChain::Ethereum | DmdChain::BinanceSmartChain | DmdChain::Polygon | 
            DmdChain::Arbitrum | DmdChain::Optimism | DmdChain::Base | DmdChain::Fantom => {
                // Kiểm tra địa chỉ EVM: 0x + 40 hex characters
                let evm_regex = &EVM_ADDRESS_REGEX;
                if !evm_regex.is_match(address) {
                    return Err(BridgeError::InvalidAddress(
                        format!("Địa chỉ EVM không hợp lệ (phải là 0x + 40 ký tự hex): {}", address)
                    ));
                }
                
                // Kiểm tra checksum cho địa chỉ EVM (đơn giản)
                if !address.chars().skip(2).any(|c| c.is_uppercase()) && 
                   !address.chars().skip(2).all(|c| c.is_lowercase()) {
                    warn!("Địa chỉ EVM có thể không có checksum đúng: {}", address);
                }
            },
            DmdChain::Solana => {
                // Kiểm tra địa chỉ Solana: base58 encoded string with 32-44 characters
                let solana_regex = &SOLANA_ADDRESS_REGEX;
                if !solana_regex.is_match(address) {
                    return Err(BridgeError::InvalidAddress(
                        format!("Địa chỉ Solana không hợp lệ (phải là chuỗi base58 độ dài 32-44 ký tự): {}", address)
                    ));
                }
            },
            _ => {
                // Kiểm tra cơ bản cho các chain khác
                if address.chars().any(|c| !c.is_alphanumeric() && c != '.' && c != '_' && c != '-') || 
                   address.len() < 5 || address.len() > 100 {
                    return Err(BridgeError::InvalidAddress(
                        format!("Địa chỉ không hợp lệ cho chain {:?}: {}", chain, address)
                    ));
                }
            },
        }
        
        Ok(())
    }
    
    /// Kiểm tra danh sách địa chỉ bị cấm
    async fn check_blocked_address(&self, address: &str, chain: &DmdChain) -> BridgeResult<()> {
        // Trong thực tế, đây sẽ là một danh sách được lấy từ DB hoặc API bên ngoài
        // Nhưng cho ví dụ, chúng ta sẽ kiểm tra một số địa chỉ cứng
        let blocked_addresses = [
            "0x000000000000000000000000000000000000dEaD", // Địa chỉ "burn" phổ biến
            "0x0000000000000000000000000000000000000000", // Địa chỉ zero
            "11111111111111111111111111111111", // Địa chỉ Solana system program
            "SysvarRent111111111111111111111111111111111", // Địa chỉ Solana rent sysvar
        ];
        
        if blocked_addresses.contains(&address) {
            return Err(BridgeError::InvalidAddress(
                format!("Địa chỉ {} bị chặn vì lý do bảo mật", address)
            ));
        }
        
        Ok(())
    }
    
    /// Kiểm tra xem địa chỉ có phải là chương trình không (chỉ áp dụng cho Solana)
    async fn is_program_address(&self, address: &str, chain: &DmdChain) -> BridgeResult<bool> {
        // Trong thực tế, sẽ gọi API của blockchain để kiểm tra
        if chain == &DmdChain::Solana {
            // Danh sách các địa chỉ chương trình hệ thống của Solana
            let system_programs = [
                "11111111111111111111111111111111", // System Program
                "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA", // SPL Token Program
                "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL", // Associated Token Account Program
            ];
            
            if system_programs.contains(&address) {
                warn!("Địa chỉ {} trên chain {:?} là địa chỉ chương trình hệ thống", address, chain);
                return Ok(true);
            }
            
            // Mô phỏng: coi như 10% địa chỉ là chương trình
            let is_program = address.as_bytes().iter().sum::<u8>() % 10 == 0;
            
            if is_program {
                warn!("Địa chỉ {} trên chain {:?} là địa chỉ chương trình", address, chain);
            }
            
            return Ok(is_program);
        }
        
        Ok(false)
    }
    
    /// Kiểm tra signature có hợp lệ không (xác thực để chống phát lại)
    async fn verify_transaction_signature(&self, signature: &str, data: &[u8]) -> BridgeResult<bool> {
        // Trong thực tế, đây sẽ xác thực chữ ký từ LayerZero Oracle
        // Giả lập xác thực thành công nếu signature có đủ độ dài
        if signature.len() < 64 {
            warn!("Chữ ký không hợp lệ (độ dài không đủ): {}", signature);
            return Ok(false);
        }
        
        // Trả về true (xác thực thành công) cho mục đích demo
        Ok(true)
    }

    /// Nhận token từ chain khác vào hub
    async fn receive_from_spoke(
        &self,
        from_chain: DmdChain,
        from_address: &str, 
        to_solana_account: &str, 
        amount: U256
    ) -> BridgeResult<BridgeTransaction> {
        // Kiểm tra provider đã được khởi tạo
        let provider = self.ensure_provider_initialized()?;
        
        // Kiểm tra xem chain có được hỗ trợ không
        if !self.supported_chains.contains(&from_chain) {
            return Err(BridgeError::UnsupportedChain(
                format!("Chain không được hỗ trợ: {:?}", from_chain)
            ));
        }
        
        // Kiểm tra LayerZero chain ID
        let lz_chain_id = match self.lz_chain_map.get(&from_chain) {
            Some(id) => *id,
            None => return Err(BridgeError::UnsupportedChain(
                format!("Chain không có LayerZero chain ID: {:?}", from_chain)
            )),
        };
        
        // Kiểm tra kỹ địa chỉ nguồn
        self.validate_address(from_address, &from_chain)?;
        
        // Kiểm tra địa chỉ có trong danh sách đen
        self.check_blocked_address(from_address, &from_chain).await?;
        
        // Kiểm tra địa chỉ nguồn có phải là hợp đồng (cho EVM) hoặc chương trình (cho Solana)
        if super::error::is_evm_chain(&from_chain) {
            // Mô phỏng kiểm tra contract address
            if from_address.as_bytes().iter().sum::<u8>() % 10 == 0 {
                warn!("Địa chỉ nguồn có thể là hợp đồng: {} trên chain {:?}", from_address, from_chain);
            }
        }
        
        // Kiểm tra kỹ địa chỉ đích (Solana account)
        self.validate_address(to_solana_account, &DmdChain::Solana)?;
        
        // Kiểm tra địa chỉ đích có trong danh sách đen
        self.check_blocked_address(to_solana_account, &DmdChain::Solana).await?;
        
        // Kiểm tra địa chỉ đích có phải là chương trình
        if self.is_program_address(to_solana_account, &DmdChain::Solana).await? {
            return Err(BridgeError::InvalidAddress(
                format!("Địa chỉ đích là chương trình, không thể nhận token: {}", to_solana_account)
            ));
        }
        
        // Kiểm tra số lượng
        if amount == U256::zero() {
            return Err(BridgeError::InvalidAmount(
                "Số lượng token phải lớn hơn 0".to_string()
            ));
        }
        
        // Kiểm tra giới hạn số lượng
        let (min_amount, max_amount) = self.get_bridge_limits(&from_chain)?;
        let amount_f64 = amount.as_u128() as f64 / 1e18;
        
        if amount_f64 < min_amount {
            return Err(BridgeError::InvalidAmount(
                format!("Số lượng quá nhỏ. Tối thiểu: {} tokens", min_amount)
            ));
        }
        
        if amount_f64 > max_amount {
            return Err(BridgeError::InvalidAmount(
                format!("Số lượng quá lớn. Tối đa: {} tokens", max_amount)
            ));
        }
        
        // Tạo giao dịch mới
        let tx = self.create_transaction(
            from_chain,
            DmdChain::Solana,
            from_address,
            to_solana_account,
            amount,
            BridgeTokenType::Erc20, // Từ ERC-20 trên EVM
            BridgeTokenType::Spl,   // Sang SPL token trên Solana
        );
        
        // Lưu giao dịch
        self.store_transaction(tx.clone()).await?;
        
        // Mô phỏng giao dịch nhận
        let tx_hash = format!("{}", Uuid::new_v4().simple());
        
        // Cập nhật thông tin giao dịch - chuyển sang trạng thái Processing
        let updated_tx = self.update_transaction(
            &tx.id,
            BridgeStatus::Processing,
            Some(tx_hash.clone()),
            None,
            None,
        ).await?;
        
        // Chờ xác nhận giao dịch với timeout được cấu hình
        let timeout_seconds = self.config.transaction_timeout_seconds.unwrap_or(300); // Mặc định 5 phút
        match self.wait_for_transaction_finality(&tx_hash, timeout_seconds).await {
            Ok(true) => {
                // Giao dịch thành công, cập nhật trạng thái
                let confirmed_tx = self.update_transaction(
                    &tx.id,
                    BridgeStatus::Completed,
                    None,
                    None,
                    None,
                ).await?;
                
                info!("Giao dịch nhận token đã hoàn thành: {}", tx.id);
                Ok(confirmed_tx)
            },
            Ok(false) => {
                // Giao dịch thất bại
                let failed_tx = self.update_transaction(
                    &tx.id,
                    BridgeStatus::Failed,
                    None,
                    None,
                    Some("Giao dịch không được xác nhận".to_string()),
                ).await?;
                
                warn!("Giao dịch nhận token thất bại: {}", tx.id);
                Ok(failed_tx)
            },
            Err(e) => {
                // Lỗi khi kiểm tra xác nhận (timeout hoặc lỗi khác)
                let err_msg = format!("Lỗi khi chờ xác nhận giao dịch: {}", e);
                let failed_tx = self.update_transaction(
                    &tx.id,
                    BridgeStatus::Failed,
                    None,
                    None,
                    Some(err_msg.clone()),
                ).await?;
                
                error!("{}", err_msg);
                Ok(failed_tx)
            }
        }
    }

    /// Gửi token từ hub sang spoke
    async fn send_to_spoke(
        &self,
        key_id: &str,
        to_chain: DmdChain,
        to_address: &str,
        amount: U256
    ) -> BridgeResult<BridgeTransaction> {
        // Validate chain
        if !self.supported_chains.contains(&to_chain) {
            return Err(BridgeError::UnsupportedRoute(
                format!("Route không được hỗ trợ: Solana -> {:?}", to_chain)
            ));
        }
        
        // Validate địa chỉ
        self.validate_address(to_address, &to_chain)?;
        
        // Kiểm tra địa chỉ bị chặn
        self.check_blocked_address(to_address, &to_chain).await?;
        
        // Đảm bảo Solana provider đã được khởi tạo
        let sol_provider = self.ensure_provider_initialized()?;
        
        // Lấy địa chỉ của operator từ config
        let from_address = &self.config.operator_address;
        
        // Kiểm tra số dư SOL - để đảm bảo đủ SOL cho phí giao dịch
        if !self.check_solana_balance(from_address).await? {
            return Err(BridgeError::InsufficientBalance(
                "Không đủ SOL để trả phí giao dịch".to_string()
            ));
        }
        
        // Kiểm tra số dư token
        let token_balance = sol_provider.get_token_balance(from_address).await
            .context("Lỗi khi lấy số dư token")
            .map_err(|e| BridgeError::ProviderError(e.to_string()))?;
            
        if token_balance < amount {
            return Err(BridgeError::InsufficientBalance(
                format!("Số dư token không đủ: có {}, cần {}", token_balance, amount)
            ));
        }
        
        // Ước tính phí
        let fee = self.estimate_fee(to_chain, amount).await?;
        
        // Kiểm tra giới hạn bridge
        let (min_amount, max_amount) = self.get_bridge_limits(&to_chain)?;
        let amount_float = amount.as_u64() as f64 / 1e9; // Giả sử 9 decimals cho token
        
        if amount_float < min_amount {
            return Err(BridgeError::InvalidAmount(
                format!("Số lượng bridge ({}) nhỏ hơn giới hạn tối thiểu ({})", amount_float, min_amount)
            ));
        }
        
        if amount_float > max_amount {
            return Err(BridgeError::InvalidAmount(
                format!("Số lượng bridge ({}) lớn hơn giới hạn tối đa ({})", amount_float, max_amount)
            ));
        }
        
        // Tạo giao dịch mới
        let tx = self.create_transaction(
            DmdChain::Solana,
            to_chain,
            from_address,
            to_address,
            amount,
            BridgeTokenType::Native,  // Solana native token
            BridgeTokenType::ERC20,   // EVM chain token
        );
        
        // Lưu giao dịch vào cache
        self.store_transaction(tx.clone()).await?;
        
        // Lấy LayerZero chain ID cho chain đích
        let lz_chain_id = self.get_lz_chain_id(to_chain)?;
        
        // Lấy khóa riêng tư từ vault để ký giao dịch
        let mut secure_key = self.config.get_operator_key_for_signing().await
            .map_err(|e| BridgeError::SecurityError(e))?;
            
        // Lấy khóa để ký và đảm bảo xóa sau khi sử dụng
        let private_key_str = secure_key.expose_for_signing()
            .map_err(|e| BridgeError::SecurityError(e))?;
            
        // Gửi giao dịch đến Solana
        let tx_hash = sol_provider.bridge_to_evm(
            private_key_str,  // Sử dụng khóa đã lấy từ vault
            lz_chain_id,
            to_address,
            amount.as_u64()
        ).await
        .context("Lỗi khi bridge token đến EVM")
        .map_err(|e| BridgeError::TransactionError(e.to_string()))?;
        
        // Xóa khóa khỏi bộ nhớ ngay lập tức sau khi sử dụng
        secure_key.clear();
        
        // Cập nhật thông tin giao dịch
        let updated_tx = self.update_transaction(
            &tx.id,
            BridgeStatus::Pending,
            Some(tx_hash.clone()),
            None,
            None
        ).await?;
        
        // Chờ giao dịch hoàn thành
        tokio::spawn({
            let this = self.clone();
            let tx_id = tx.id.clone();
            let tx_hash = tx_hash.clone();
            
            async move {
                // Chờ giao dịch được xác nhận trên Solana
                match this.wait_for_transaction_finality(&tx_hash, this.config.confirmation_timeout).await {
                    Ok(true) => {
                        // Giao dịch thành công, đang chờ nhận token ở chain đích
                        let _ = this.update_transaction(
                            &tx_id,
                            BridgeStatus::Processing,
                            None,
                            None,
                            None
                        ).await;
                        
                        // Ở đây sẽ cần một cơ chế để kiểm tra khi nào token đến chain đích
                        // và cập nhật trạng thái giao dịch thành Completed
                    }
                    Ok(false) => {
                        // Giao dịch timeout
                        let _ = this.update_transaction(
                            &tx_id,
                            BridgeStatus::Failed,
                            None,
                            None,
                            Some("Giao dịch timeout".to_string())
                        ).await;
                    }
                    Err(e) => {
                        // Lỗi khi chờ giao dịch
                        let _ = this.update_transaction(
                            &tx_id,
                            BridgeStatus::Failed,
                            None,
                            None,
                            Some(format!("Lỗi khi chờ giao dịch: {}", e))
                        ).await;
                    }
                }
            }
        });
        
        Ok(updated_tx)
    }
}

#[async_trait]
impl BridgeHub for SolBridgeHub {
    /// Khởi tạo hub với cấu hình cho trước
    async fn initialize(&mut self, config: BridgeConfig) -> BridgeResult<()> {
        self.config = config.clone();
        
        // Khởi tạo Solana provider
        let sol_provider = SolanaProgramProvider::new(None)
            .await
            .map_err(|e| BridgeError::ProviderError(format!("Không thể tạo Solana provider: {}", e)))?;
        
        self.sol_provider = Some(Arc::new(sol_provider));
        
        // Nếu cấu hình có chỉ định file lưu trữ, tạo repository
        if let Some(storage_path) = &config.transaction_storage_path {
            self.init_default_repository(storage_path)?;
        }
        
        info!("Solana bridge hub đã khởi tạo với program ID: {}", self.config.solana_bridge_program_id);
        Ok(())
    }

    /// Kiểm tra trạng thái của một giao dịch
    async fn check_transaction_status(&self, tx_id: &str) -> BridgeResult<BridgeStatus> {
        // Kiểm tra xem giao dịch có tồn tại trong cache không
        let transactions = self.transactions.read().await;
        
        if let Some(tx) = transactions.get(tx_id) {
            return Ok(tx.status.clone());
        }
        
        // Nếu không tìm thấy trong cache, kiểm tra trong repository
        if let Some(repo) = &self.persistent_repo {
            match repo.find_by_id(tx_id) {
                Ok(Some(tx)) => {
                    // Cập nhật cache với dữ liệu từ repository
                    drop(transactions); // Giải phóng read lock trước khi lấy write lock
                    let mut txs = self.transactions.write().await;
                    txs.insert(tx_id.to_string(), tx.clone());
                    
                    return Ok(tx.status);
                },
                Ok(None) => {},
                Err(e) => {
                    warn!("Lỗi khi tìm giao dịch từ repository: {}", e);
                }
            }
        }
        
        // Nếu không tìm thấy, trả về lỗi
        Err(BridgeError::TransactionNotFound(
            format!("Không tìm thấy giao dịch với ID: {}", tx_id)
        ))
    }

    /// Lấy thông tin chi tiết của một giao dịch
    async fn get_transaction(&self, tx_id: &str) -> BridgeResult<Option<BridgeTransaction>> {
        // Kiểm tra trong cache
        let transactions = self.transactions.read().await;
        
        if let Some(tx) = transactions.get(tx_id) {
            return Ok(Some(tx.clone()));
        }
        
        // Nếu không tìm thấy trong cache, kiểm tra trong repository
        if let Some(repo) = &self.persistent_repo {
            match repo.find_by_id(tx_id) {
                Ok(Some(tx)) => {
                    // Cập nhật cache với dữ liệu từ repository
                    drop(transactions); // Giải phóng read lock trước khi lấy write lock
                    let mut txs = self.transactions.write().await;
                    txs.insert(tx_id.to_string(), tx.clone());
                    
                    return Ok(Some(tx));
                },
                Ok(None) => {},
                Err(e) => {
                    warn!("Lỗi khi tìm giao dịch từ repository: {}", e);
                }
            }
        }
        
        // Không tìm thấy giao dịch
        Ok(None)
    }

    /// Ước tính phí giao dịch cho việc chuyển token
    async fn estimate_fee(&self, to_chain: DmdChain, amount: U256) -> BridgeResult<U256> {
        // Đảm bảo provider đã được khởi tạo
        let _provider = self.ensure_provider_initialized()?;
        
        // Kiểm tra xem chain có được hỗ trợ không
        if !self.supported_chains.contains(&to_chain) {
            return Err(BridgeError::UnsupportedChain(
                format!("Chain đích không được hỗ trợ: {:?}", to_chain)
            ));
        }
        
        // Tính toán phí cơ bản dựa trên cấu hình
        let base_fee = self.config.base_fee.unwrap_or(U256::from(0.01 * 1e18 as f64));
        
        // Tính toán phí dựa trên số lượng token và chain đích
        let amount_fee_multiplier = match to_chain {
            DmdChain::Ethereum => 0.001, // 0.1%
            DmdChain::Bsc => 0.0005,     // 0.05%
            DmdChain::Polygon => 0.0005, // 0.05%
            DmdChain::Arbitrum => 0.001, // 0.1%
            DmdChain::Near => 0.001,     // 0.1%
            _ => 0.002,                  // 0.2% cho các chain khác
        };
        
        // Chuyển đổi amount sang f64 để tính toán
        let amount_f64 = amount.as_u128() as f64 / 1e18;
        let fee_from_amount = U256::from((amount_f64 * amount_fee_multiplier * 1e18) as u128);
        
        // Phí cuối cùng là tổng của phí cơ bản và phí dựa trên số lượng
        let total_fee = base_fee + fee_from_amount;
        
        // Phí tối thiểu
        let min_fee = U256::from(0.005 * 1e18 as f64);
        
        if total_fee < min_fee {
            Ok(min_fee)
        } else {
            Ok(total_fee)
        }
    }

    /// Lấy danh sách các chain được hỗ trợ bởi hub
    async fn get_supported_chains(&self) -> BridgeResult<Vec<DmdChain>> {
        Ok(self.supported_chains.clone())
    }

    /// Xóa cache của các giao dịch cũ dựa trên thời gian hoặc số lượng
    async fn cleanup_transaction_cache(&self, max_age: Option<Duration>, max_count: Option<usize>) -> BridgeResult<usize> {
        let now = SystemTime::now();
        let mut transactions = self.transactions.write().await;
        
        let original_count = transactions.len();
        
        // Danh sách các ID giao dịch cần xóa
        let mut to_remove = Vec::new();
        
        // Kiểm tra giao dịch dựa trên thời gian
        if let Some(age) = max_age {
            for (id, tx) in transactions.iter() {
                if let Ok(tx_time) = DateTime::parse_from_rfc3339(&tx.timestamp) {
                    let tx_system_time = SystemTime::from(tx_time.with_timezone(&Utc));
                    
                    // Nếu giao dịch quá cũ, thêm vào danh sách cần xóa
                    if let Ok(duration) = now.duration_since(tx_system_time) {
                        if duration > age {
                            to_remove.push(id.clone());
                        }
                    }
                }
            }
        }
        
        // Xóa các giao dịch quá cũ khỏi cache
        for id in &to_remove {
            transactions.remove(id);
        }
        
        // Nếu cần giới hạn số lượng giao dịch tối đa
        if let Some(count) = max_count {
            if transactions.len() > count {
                // Sắp xếp giao dịch theo thời gian, giữ lại những giao dịch mới nhất
                let mut tx_vec: Vec<(String, BridgeTransaction)> = transactions
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                
                // Sắp xếp theo thời gian, mới nhất lên đầu
                tx_vec.sort_by(|(_, a), (_, b)| b.timestamp.cmp(&a.timestamp));
                
                // Giữ lại chỉ count giao dịch mới nhất
                transactions.clear();
                
                for (i, (id, tx)) in tx_vec.into_iter().enumerate() {
                    if i < count {
                        transactions.insert(id, tx);
                    } else {
                        to_remove.push(id);
                    }
                }
            }
        }
        
        let removed_count = original_count - transactions.len();
        
        // Xóa các giao dịch từ repository nếu có
        if !to_remove.is_empty() && self.persistent_repo.is_some() {
            let repo = self.persistent_repo.as_ref().unwrap();
            
            // Xóa từng giao dịch
            for id in &to_remove {
                if let Err(e) = repo.delete_by_id(id) {
                    warn!("Không thể xóa giao dịch {} từ repository: {}", id, e);
                }
            }
        }
        
        // Ghi log số lượng giao dịch đã xóa
        if removed_count > 0 {
            info!("Đã xóa {} giao dịch khỏi cache SolBridgeHub", removed_count);
        }
        
        Ok(removed_count)
    }

    /// Quản lý cache giao dịch tự động dựa trên ngưỡng
    async fn manage_cache(&self) -> BridgeResult<()> {
        // Lấy cấu hình từ config hoặc sử dụng giá trị mặc định
        let max_age = self.config.transaction_cache_max_age
            .map(|hours| Duration::from_secs(hours * 3600))
            .unwrap_or(Duration::from_secs(24 * 3600)); // Mặc định 24 giờ
        
        let max_count = self.config.transaction_cache_max_count.unwrap_or(1000); // Mặc định 1000 giao dịch
        
        // Đếm số lượng giao dịch hiện tại
        let current_count = {
            let transactions = self.transactions.read().await;
            transactions.len()
        };
        
        // Nếu đạt ngưỡng số lượng giao dịch, thực hiện dọn dẹp
        if current_count > max_count * 8 / 10 { // Dọn dẹp khi đạt 80% ngưỡng
            let removed = self.cleanup_transaction_cache(Some(max_age), Some(max_count)).await?;
            
            info!(
                "Quản lý cache tự động: đã xóa {} giao dịch, còn lại {} giao dịch",
                removed,
                {
                    let transactions = self.transactions.read().await;
                    transactions.len()
                }
            );
        }
        
        // Đồng bộ dữ liệu trong repository nếu có
        if let Some(repo) = &self.persistent_repo {
            if let Err(e) = repo.flush() {
                warn!("Không thể ghi dữ liệu từ cache xuống storage: {}", e);
            } else {
                debug!("Đã đồng bộ cache với persistent storage");
            }
        }
        
        Ok(())
    }

    async fn send_tokens_to_chain(
        &self,
        key_id: &str,
        to_chain: DmdChain,
        to_address: &str,
        amount: U256
    ) -> BridgeResult<BridgeTransaction> {
        self.send_to_spoke(key_id, to_chain, to_address, amount).await
    }
}

// Thêm phương thức để kiểm tra số dư ví Solana
impl SolBridgeHub {
    /// Kiểm tra số dư ví Solana
    async fn check_solana_balance(&self, address: &str) -> BridgeResult<bool> {
        let provider = self.ensure_provider_initialized()?;
        
        // Truy vấn số dư từ provider
        let balance = provider.get_balance(address)
            .await
            .map_err(|e| BridgeError::ProviderError(format!("Không thể lấy số dư ví Solana: {}", e)))?;
        
        // Chuyển đổi balance sang U256 để so sánh
        let balance_u256 = U256::from(balance);
        
        // Kiểm tra xem số dư có đủ không (ví dụ: phí giao dịch tối thiểu)
        let min_required = U256::from(0.001 * 1e9 as f64); // 0.001 SOL ở đơn vị lamports
        
        Ok(balance_u256 >= min_required)
    }
    
    /// Lấy giới hạn bridge cho từng cặp chain
    fn get_bridge_limits(&self, to_chain: &DmdChain) -> BridgeResult<(f64, f64)> {
        // Giới hạn số lượng token tối thiểu và tối đa cho từng cặp chain
        let (min_amount, max_amount) = match to_chain {
            DmdChain::Ethereum => (0.01, 1000.0),   // Min: 0.01 tokens, Max: 1000 tokens
            DmdChain::Bsc => (0.01, 10000.0),       // Min: 0.01 tokens, Max: 10000 tokens  
            DmdChain::Polygon => (0.01, 10000.0),   // Min: 0.01 tokens, Max: 10000 tokens
            DmdChain::Arbitrum => (0.01, 1000.0),   // Min: 0.01 tokens, Max: 1000 tokens
            DmdChain::Near => (0.1, 1000.0),        // Min: 0.1 tokens, Max: 1000 tokens
            DmdChain::Solana => (0.1, 10000.0),     // Min: 0.1 tokens, Max: 10000 tokens
            _ => (1.0, 100.0),                      // Min: 1 token, Max: 100 tokens cho các chain khác
        };
        
        Ok((min_amount, max_amount))
    }
}
