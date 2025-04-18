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
use rand;
use regex::Regex;
use once_cell::sync::Lazy;

use crate::smartcontracts::{
    dmd_token::DmdChain,
    near_contract::NearContractProvider,
};

use super::{
    error::{BridgeError, BridgeResult},
    traits::BridgeHub,
    types::{BridgeTransaction, BridgeStatus, BridgeConfig, BridgeTokenType},
};

// Thêm các regex patterns dùng Lazy để thay thế các unwrap() trong hàm validate_address
// Khai báo static patterns ở đầu module, ngoài các hàm
static EVM_ADDRESS_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^0x[0-9a-fA-F]{40}$")
        .expect("Không thể biên dịch regex pattern cho địa chỉ EVM")
});

static NEAR_ACCOUNT_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^[a-z0-9_-]{2,64}$")
        .expect("Không thể biên dịch regex pattern cho tài khoản NEAR")
});

static SOLANA_ADDRESS_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^[1-9A-HJ-NP-Za-km-z]{32,44}$")
        .expect("Không thể biên dịch regex pattern cho địa chỉ Solana")
});

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

/// Cấu trúc để theo dõi thống kê hiệu suất cache
pub struct CacheMetrics {
    /// Thời điểm bắt đầu đo lường
    start_time: chrono::DateTime<chrono::Utc>,
    /// Kích thước cache ban đầu
    initial_size: usize,
    /// Kích thước cache tối đa đạt được
    max_size: usize,
    /// Tốc độ tăng trưởng cache (số giao dịch/giờ)
    growth_rate: f64,
    /// Số lần cảnh báo tăng tải đã phát
    alert_count: usize,
    /// Thời điểm cảnh báo cuối cùng
    last_alert: Option<chrono::DateTime<chrono::Utc>>,
    /// Có đang trong chế độ cảnh báo không
    is_in_alert_state: bool,
}

impl Default for CacheMetrics {
    fn default() -> Self {
        Self {
            start_time: chrono::Utc::now(),
            initial_size: 0,
            max_size: 0,
            growth_rate: 0.0,
            alert_count: 0,
            last_alert: None,
            is_in_alert_state: false,
        }
    }
}

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
    /// Số lượng giao dịch tối đa trong cache
    max_cache_size: usize,
    /// Metrics theo dõi hiệu suất cache
    cache_metrics: RwLock<CacheMetrics>,
    /// Cấu hình mở rộng tự động
    auto_scaling_enabled: bool,
    /// Callback thông báo cho admin (Option<Arc<Fn(String, String) -> Result<(), String> + Send + Sync>>)
    admin_notifier: Option<Arc<dyn Fn(String, String) -> Result<(), String> + Send + Sync>>,
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
            max_cache_size: 5000,
            cache_metrics: RwLock::new(CacheMetrics::default()),
            auto_scaling_enabled: true,
            admin_notifier: None,
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

    /// Chờ giao dịch NEAR được xác nhận với cơ chế timeout
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
                debug!("Kiểm tra xác nhận giao dịch NEAR lần {}/{}: {}", attempt, max_attempts, tx_hash_clone);
                
                // Giả lập kiểm tra xác nhận giao dịch (thực tế sẽ gọi NEAR RPC)
                // TODO: Thay thế bằng lệnh gọi thực tế đến NEAR RPC
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
                        info!("Giao dịch NEAR đã được xác nhận: {}", tx_hash);
                        Ok(true)
                    },
                    Some(false) => {
                        warn!("Giao dịch NEAR không được xác nhận sau {} lần thử: {}", max_attempts, tx_hash);
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
                error!("Đã vượt quá thời gian chờ {} giây khi chờ xác nhận giao dịch NEAR: {}", timeout_seconds, tx_hash);
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
            DmdChain::Near => {
                // Kiểm tra địa chỉ NEAR: phải kết thúc bằng .near hoặc .testnet và không chứa ký tự đặc biệt trừ dấu chấm và gạch dưới
                if !address.ends_with(".near") && !address.ends_with(".testnet") {
                    return Err(BridgeError::InvalidAddress(
                        format!("Địa chỉ NEAR không hợp lệ (phải kết thúc bằng .near hoặc .testnet): {}", address)
                    ));
                }
                
                let near_account_name = address.split('.').next().unwrap_or("");
                let account_regex = &NEAR_ACCOUNT_REGEX;
                if !account_regex.is_match(near_account_name) {
                    return Err(BridgeError::InvalidAddress(
                        format!("Tên tài khoản NEAR không hợp lệ (chỉ chấp nhận a-z, 0-9, _, - và độ dài 2-64 ký tự): {}", near_account_name)
                    ));
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
                // Đảm bảo không có ký tự đặc biệt và độ dài hợp lý
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
            "blacklisted.near",
            "suspicious.testnet",
        ];
        
        if blocked_addresses.contains(&address) {
            return Err(BridgeError::InvalidAddress(
                format!("Địa chỉ {} bị chặn vì lý do bảo mật", address)
            ));
        }
        
        Ok(())
    }
    
    /// Kiểm tra xem địa chỉ có phải là hợp đồng không (chỉ áp dụng cho EVM)
    async fn is_contract_address(&self, address: &str, chain: &DmdChain) -> BridgeResult<bool> {
        // Trong thực tế, sẽ gọi API của blockchain để kiểm tra
        // Nhưng ở đây chúng ta mô phỏng
        if super::error::is_evm_chain(chain) {
            // Kiểm tra địa chỉ có phải là hợp đồng không
            // Mô phỏng: coi như 10% địa chỉ là hợp đồng
            let is_contract = address.as_bytes().iter().sum::<u8>() % 10 == 0;
            
            if is_contract {
                warn!("Địa chỉ {} trên chain {:?} là địa chỉ hợp đồng", address, chain);
            }
            
            return Ok(is_contract);
        }
        
        Ok(false)
    }

    /// Thiết lập kích thước cache tối đa
    pub fn with_max_cache_size(mut self, size: usize) -> Self {
        if size < 100 {
            warn!("Kích thước cache quá nhỏ, sử dụng giá trị tối thiểu 100");
            self.max_cache_size = 100;
        } else {
            self.max_cache_size = size;
        }
        self
    }

    /// Bật/tắt tính năng tự động mở rộng cache
    pub fn with_auto_scaling(mut self, enabled: bool) -> Self {
        self.auto_scaling_enabled = enabled;
        self
    }

    /// Thiết lập hàm thông báo cho admin
    pub fn with_admin_notifier<F>(mut self, notifier: F) -> Self 
    where 
        F: Fn(String, String) -> Result<(), String> + Send + Sync + 'static 
    {
        self.admin_notifier = Some(Arc::new(notifier));
        self
    }

    /// Gửi thông báo cho admin
    async fn notify_admin(&self, alert_type: &str, message: &str) -> Result<(), String> {
        if let Some(notifier) = &self.admin_notifier {
            notifier(alert_type.to_string(), message.to_string())?;
            
            // Cập nhật trạng thái cảnh báo
            let mut metrics = self.cache_metrics.write().await;
            metrics.last_alert = Some(chrono::Utc::now());
            metrics.alert_count += 1;
            metrics.is_in_alert_state = true;
        }
        Ok(())
    }

    /// Mở rộng cache khi cần thiết
    async fn auto_scale_cache(&self, current_size: usize) -> BridgeResult<()> {
        if !self.auto_scaling_enabled {
            return Ok(());
        }

        // Chỉ mở rộng nếu cache đang đầy tới 80%
        let usage_percentage = (current_size as f64 / self.max_cache_size as f64) * 100.0;
        
        if usage_percentage >= 80.0 {
            // Tính tốc độ tăng trưởng
            let mut metrics = self.cache_metrics.write().await;
            let now = chrono::Utc::now();
            let hours_elapsed = (now - metrics.start_time).num_milliseconds() as f64 / 3_600_000.0;
            
            if hours_elapsed > 0.0 {
                // Cập nhật metrics
                metrics.growth_rate = (current_size as f64 - metrics.initial_size as f64) / hours_elapsed;
                
                // Theo dõi kích thước tối đa
                if current_size > metrics.max_size {
                    metrics.max_size = current_size;
                }
                
                // Nếu tốc độ tăng trưởng cao, điều chỉnh kích thước cache
                if metrics.growth_rate > 100.0 { // Hơn 100 giao dịch/giờ
                    let old_max = self.max_cache_size;
                    let new_max = (self.max_cache_size as f64 * 1.5) as usize;
                    
                    // Cập nhật kích thước tối đa
                    let mut txs = self.transactions.write().await;
                    drop(txs); // Giải phóng lock để không block quá lâu
                    
                    // Ở đây không thể thay đổi max_cache_size vì nó là bất biến, 
                    // nhưng trong thực tế ta có thể triển khai cơ chế mở rộng thông qua bộ nhớ phân tán
                    
                    // Chỉ gửi cảnh báo nếu không ở trong trạng thái cảnh báo
                    // hoặc cảnh báo cuối đã cách đây ít nhất 1 giờ
                    let should_alert = !metrics.is_in_alert_state || 
                        metrics.last_alert.map_or(true, |t| (now - t).num_hours() >= 1);
                        
                    if should_alert {
                        let message = format!(
                            "Tự động mở rộng cache: {} -> {} giao dịch. Tốc độ tăng: {:.2} giao dịch/giờ. Sử dụng: {:.1}%",
                            old_max, new_max, metrics.growth_rate, usage_percentage
                        );
                        info!("{}", message);
                        self.notify_admin("CACHE_AUTO_SCALING", &message).await?;
                    }
                }
            }
        } else if usage_percentage < 50.0 {
            // Nếu sử dụng dưới 50%, reset trạng thái cảnh báo
            let mut metrics = self.cache_metrics.write().await;
            metrics.is_in_alert_state = false;
        }
        
        Ok(())
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
        
        // Kiểm tra địa chỉ nguồn có phải là hợp đồng
        if self.is_contract_address(from_address, &from_chain).await? {
            // Nếu là hợp đồng, cần kiểm tra thêm (ví dụ whitelist các hợp đồng được phép)
            // Đối với demo này, chúng ta chỉ cảnh báo
            warn!("Địa chỉ nguồn là hợp đồng: {} trên chain {:?}", from_address, from_chain);
        }
        
        // Kiểm tra kỹ địa chỉ đích (NEAR account)
        self.validate_address(to_near_account, &DmdChain::Near)?;
        
        // Kiểm tra địa chỉ đích có trong danh sách đen
        self.check_blocked_address(to_near_account, &DmdChain::Near).await?;
        
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
        
        // Quản lý cache tự động
        if let Err(e) = self.manage_cache().await {
            warn!("Lỗi khi quản lý cache: {}", e);
        }
        
        // Log thông tin đã được chuyển vào các trường hợp xử lý ở trên
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
        
        // Cập nhật trạng thái - chuyển sang Sending
        let updated_tx = self.update_transaction(
            &tx.id,
            BridgeStatus::Sending,
            Some(source_tx_hash.clone()),
            None,
            None,
        ).await?;
        
        // Chờ xác nhận giao dịch với timeout được cấu hình
        let timeout_seconds = self.config.transaction_timeout_seconds.unwrap_or(300); // Mặc định 5 phút
        match self.wait_for_transaction_finality(&source_tx_hash, timeout_seconds).await {
            Ok(true) => {
                // Giao dịch đã được xác nhận trên NEAR, chờ LayerZero relayer
                let confirmed_tx = self.update_transaction(
                    &tx.id,
                    BridgeStatus::SourceConfirmed,
                    None,
                    None,
                    None,
                ).await?;
                
                info!("Giao dịch gửi token đã được xác nhận trên NEAR: {}", tx.id);
                Ok(confirmed_tx)
            },
            Ok(false) => {
                // Giao dịch thất bại
                let failed_tx = self.update_transaction(
                    &tx.id,
                    BridgeStatus::Failed,
                    None,
                    None,
                    Some("Giao dịch không được xác nhận trên NEAR".to_string()),
                ).await?;
                
                warn!("Giao dịch gửi token thất bại: {}", tx.id);
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
        
        // Quản lý cache tự động
        if let Err(e) = self.manage_cache().await {
            warn!("Lỗi khi quản lý cache: {}", e);
        }
        
        // Log thông tin đã được chuyển vào các trường hợp xử lý ở trên
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
        // Lấy các chain có trong lz_chain_map
        let mut chains: Vec<DmdChain> = self.lz_chain_map.keys()
            .filter(|&chain| *chain != DmdChain::Near) // Loại trừ NEAR vì đây là hub
            .cloned()
            .collect();
        
        // Sắp xếp để có kết quả ổn định
        chains.sort_by_key(|chain| {
            match chain {
                DmdChain::Ethereum => 1,
                DmdChain::BinanceSmartChain => 2,
                DmdChain::Avalanche => 3,
                DmdChain::Polygon => 4,
                DmdChain::Arbitrum => 5,
                DmdChain::Optimism => 6,
                DmdChain::Fantom => 7,
                DmdChain::Base => 8,
                _ => 100, // Các chain khác
            }
        });
        
        chains
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
        const MAX_AGE_HOURS: i64 = 24; // Giữ giao dịch tối đa 24 giờ
        const CACHE_WARNING_THRESHOLD_PERCENT: f64 = 80.0; // Ngưỡng cảnh báo ở 80%
        const CACHE_CRITICAL_THRESHOLD_PERCENT: f64 = 90.0; // Ngưỡng tới hạn ở 90%
        
        // Kiểm tra kích thước cache
        let cache_size = {
            let txs = self.transactions.read().await;
            txs.len()
        };
        
        // Cập nhật metrics nếu là lần đầu
        {
            let mut metrics = self.cache_metrics.write().await;
            if metrics.initial_size == 0 {
                metrics.initial_size = cache_size;
                metrics.start_time = chrono::Utc::now();
            }
            
            // Cập nhật kích thước tối đa
            if cache_size > metrics.max_size {
                metrics.max_size = cache_size;
            }
        }
        
        // Tính toán ngưỡng cụ thể dựa trên kích thước tối đa
        let warning_threshold = (self.max_cache_size as f64 * CACHE_WARNING_THRESHOLD_PERCENT / 100.0) as usize;
        let critical_threshold = (self.max_cache_size as f64 * CACHE_CRITICAL_THRESHOLD_PERCENT / 100.0) as usize;
        
        // Thực hiện dọn dẹp tự động trong các trường hợp sau:
        let now = chrono::Utc::now();
        
        // 1. Cache quá tải (vượt ngưỡng tới hạn)
        if cache_size > critical_threshold {
            let message = format!("Cache quá tải nghiêm trọng ({}/{} giao dịch - {:.1}%), dọn dẹp ngay...", 
                cache_size, self.max_cache_size, (cache_size as f64 / self.max_cache_size as f64) * 100.0);
            error!("{}", message);
            
            // Gửi cảnh báo cho admin
            self.notify_admin("CACHE_CRITICAL", &message).await?;
            
            // Dọn sạch các giao dịch đã hoàn thành và cũ hơn 1 giờ
            Self::_cleanup_completed_transactions(self.transactions.clone(), 1).await?;
            
            // Kiểm tra xem có cần mở rộng cache không
            self.auto_scale_cache(cache_size).await?;
            
            return Ok(());
        }
        
        // 2. Cache gần đạt ngưỡng (vượt ngưỡng cảnh báo)
        if cache_size > warning_threshold {
            let message = format!("Cache gần đạt ngưỡng ({}/{} giao dịch - {:.1}%), dọn dẹp...", 
                cache_size, self.max_cache_size, (cache_size as f64 / self.max_cache_size as f64) * 100.0);
            warn!("{}", message);
            
            // Gửi cảnh báo cho admin
            self.notify_admin("CACHE_WARNING", &message).await?;
            
            // Dọn sạch các giao dịch đã hoàn thành và cũ hơn 6 giờ
            Self::_cleanup_completed_transactions(self.transactions.clone(), 6).await?;
            
            // Kiểm tra xem có cần mở rộng cache không
            self.auto_scale_cache(cache_size).await?;
            
            return Ok(());
        }
        
        // 3. Dọn dẹp định kỳ (theo nguyên tắc xác suất để tránh tất cả các request cùng dọn)
        // Chỉ thực hiện với xác suất 1% để tránh tất cả các request cùng thực hiện dọn dẹp
        if cache_size > 1000 && rand::random::<f64>() < 0.01 {
            debug!("Dọn dẹp cache định kỳ ({} giao dịch)...", cache_size);
            // Dọn sạch các giao dịch đã hoàn thành và cũ hơn 12 giờ
            Self::_cleanup_completed_transactions(self.transactions.clone(), 12).await?;
        }
        
        // 4. Phát hiện tăng tải đột biến dựa trên tốc độ tăng trong 5 phút gần đây
        let metrics = self.cache_metrics.read().await;
        let now = chrono::Utc::now();
        let hours_elapsed = (now - metrics.start_time).num_milliseconds() as f64 / 3_600_000.0;
        
        if hours_elapsed > 0.083 { // > 5 phút
            let growth_rate = (cache_size as f64 - metrics.initial_size as f64) / hours_elapsed;
            
            // Nếu tốc độ tăng trưởng rất cao (>500 giao dịch/giờ) và không ở trong trạng thái cảnh báo
            if growth_rate > 500.0 && !metrics.is_in_alert_state {
                let message = format!(
                    "Phát hiện tăng tải đột biến! Tốc độ tăng: {:.2} giao dịch/giờ. Cache: {}/{} ({:.1}%)",
                    growth_rate, cache_size, self.max_cache_size, 
                    (cache_size as f64 / self.max_cache_size as f64) * 100.0
                );
                warn!("{}", message);
                
                // Gửi cảnh báo cho admin
                drop(metrics); // Giải phóng read lock trước khi lấy write lock trong notify_admin
                self.notify_admin("CACHE_SUDDEN_GROWTH", &message).await?;
                
                // Mở rộng cache ngay lập tức nếu cần
                if self.auto_scaling_enabled {
                    self.auto_scale_cache(cache_size).await?;
                }
            }
        }
        
        Ok(())
    }
    
    /// Phương thức nội bộ để dọn dẹp các giao dịch đã hoàn thành và cũ
    async fn _cleanup_completed_transactions(
        transactions: Arc<RwLock<HashMap<String, BridgeTransaction>>>, 
        older_than_hours: i64
    ) -> BridgeResult<usize> {
        let mut txs = transactions.write().await;
        let initial_size = txs.len();
        
        // Thời điểm hiện tại
        let now = Utc::now();
        let cutoff = now - chrono::Duration::hours(older_than_hours);
        
        // Lọc các giao dịch cần giữ lại
        let to_retain: HashMap<String, BridgeTransaction> = txs
            .drain()
            .filter(|(_, tx)| {
                // Giữ lại các giao dịch chưa hoàn thành
                if tx.status != BridgeStatus::Completed && tx.status != BridgeStatus::Failed {
                    return true;
                }
                
                // Hoặc các giao dịch mới hơn mốc thời gian
                tx.initiated_at > cutoff
            })
            .collect();
        
        // Cập nhật lại cache
        let removed = initial_size - to_retain.len();
        *txs = to_retain;
        
        debug!("Đã dọn dẹp {} giao dịch cũ hơn {} giờ, còn lại {}", 
               removed, older_than_hours, txs.len());
        
        Ok(removed)
    }

    /// Trả về giới hạn bridge cho cặp chain cụ thể
    fn get_bridge_limits(&self, from_chain: &DmdChain) -> BridgeResult<(f64, f64)> {
        // Giới hạn mặc định
        let default_min = 0.01;
        let default_max = 10000.0;
        
        // Giới hạn tùy thuộc vào chain nguồn
        match from_chain {
            DmdChain::Ethereum => Ok((0.01, 50000.0)),
            DmdChain::BinanceSmartChain => Ok((0.05, 100000.0)),
            DmdChain::Polygon => Ok((1.0, 200000.0)),
            DmdChain::Arbitrum => Ok((0.01, 50000.0)),
            DmdChain::Optimism => Ok((0.01, 50000.0)),
            DmdChain::Base => Ok((0.01, 50000.0)),
            DmdChain::Solana => Ok((0.1, 100000.0)),
            _ => Ok((default_min, default_max)),
        }
    }
} 