//! Module quản lý cấu hình cho bridge, chú trọng bảo mật khóa riêng tư

use crate::smartcontracts::dmd_token::DmdChain;
use std::collections::HashMap;
use secrecy::{Secret, SecretString, ExposeSecret};
use zeroize::Zeroize;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;

/// Số lần tối đa cho phép truy cập khóa riêng tư
const MAX_KEY_ACCESS_COUNT: usize = 5;
/// Thời gian hết hạn của khóa riêng tư sau khi load (tính bằng giây)
const KEY_EXPIRY_SECONDS: u64 = 300; // 5 phút

/// Các mã lỗi liên quan đến khóa
#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    /// Vault chưa được khởi tạo
    #[error("Key vault chưa được khởi tạo")]
    VaultNotInitialized,
    
    /// Không tìm thấy khóa
    #[error("Không tìm thấy khóa: {0}")]
    KeyNotFound(String),
    
    /// Khóa đã hết hạn
    #[error("Khóa đã hết hạn: {0}")]
    KeyExpired(String),
    
    /// Vượt quá số lần truy cập khóa
    #[error("Vượt quá số lần truy cập khóa: {0}")]
    MaxAccessExceeded(String),
    
    /// Lỗi khi truy cập vault
    #[error("Lỗi truy cập vault: {0}")]
    VaultAccessError(String),
    
    /// Ghi đè khóa
    #[error("Khóa đã tồn tại: {0}")]
    KeyAlreadyExists(String),
    
    /// Lỗi xóa khóa
    #[error("Lỗi xóa khóa: {0}")]
    KeyDeleteError(String),
    
    /// Lỗi khác
    #[error("Lỗi khác: {0}")]
    Other(String),
}

impl From<String> for KeyError {
    fn from(msg: String) -> Self {
        KeyError::Other(msg)
    }
}

impl From<&str> for KeyError {
    fn from(msg: &str) -> Self {
        KeyError::Other(msg.to_string())
    }
}

// Định nghĩa Result cho các hàm liên quan đến khóa
pub type KeyResult<T> = Result<T, KeyError>;

/// Cấu trúc bọc khóa riêng tư với khả năng tự động xóa khi bị hủy
/// và tính năng theo dõi sử dụng
#[derive(Clone, Zeroize)]
#[zeroize(drop)]
pub struct SecurePrivateKey {
    /// Khóa riêng tư được mã hóa
    key: SecretString,
    /// ID của khóa để theo dõi
    key_id: String,
    /// Số lần đã truy cập khóa
    access_count: usize,
    /// Thời điểm khóa sẽ hết hạn
    expiry: Instant,
}

impl SecurePrivateKey {
    /// Tạo một khóa riêng tư bảo mật mới
    pub fn new(key_id: &str, key: &str) -> Self {
        Self {
            key: SecretString::new(key.to_string()),
            key_id: key_id.to_string(),
            access_count: 0,
            expiry: Instant::now() + Duration::from_secs(KEY_EXPIRY_SECONDS),
        }
    }
    
    /// Lấy giá trị khóa (chỉ nên sử dụng khi cần thiết)
    /// 
    /// # Safety
    /// Hàm này làm lộ khóa riêng tư và chỉ nên được sử dụng trong
    /// ngữ cảnh ký giao dịch. Hàm này theo dõi số lần sử dụng và
    /// thời gian hết hạn để tăng bảo mật.
    pub fn expose_for_signing(&mut self) -> KeyResult<&str> {
        // Kiểm tra số lần truy cập
        if self.access_count >= MAX_KEY_ACCESS_COUNT {
            return Err(KeyError::MaxAccessExceeded(self.key_id.clone()));
        }
        
        // Kiểm tra thời gian hết hạn
        if Instant::now() > self.expiry {
            // Xóa khóa và trả về lỗi
            self.zeroize();
            return Err(KeyError::KeyExpired(self.key_id.clone()));
        }
        
        // Tăng số lần truy cập
        self.access_count += 1;
        
        // Ghi log về việc sử dụng khóa (không bao gồm khóa!)
        warn!("Khóa riêng tư '{}' đã được truy cập {} / {} lần", 
            self.key_id, self.access_count, MAX_KEY_ACCESS_COUNT);
        
        Ok(self.key.expose_secret())
    }
    
    /// Kiểm tra xem khóa có còn hiệu lực không
    pub fn is_valid(&self) -> bool {
        self.access_count < MAX_KEY_ACCESS_COUNT && Instant::now() <= self.expiry
    }
    
    /// Gia hạn khóa, thiết lập lại thời gian hết hạn
    pub fn renew(&mut self) {
        self.expiry = Instant::now() + Duration::from_secs(KEY_EXPIRY_SECONDS);
        debug!("Khóa riêng tư '{}' đã được gia hạn thêm {} giây", 
            self.key_id, KEY_EXPIRY_SECONDS);
    }
    
    /// Xóa khóa khỏi bộ nhớ ngay lập tức
    pub fn clear(&mut self) {
        self.zeroize();
        debug!("Khóa riêng tư '{}' đã được xóa khỏi bộ nhớ", self.key_id);
    }
    
    /// Lấy ID của khóa
    pub fn key_id(&self) -> &str {
        &self.key_id
    }
    
    /// Lấy số lần đã truy cập khóa
    pub fn access_count(&self) -> usize {
        self.access_count
    }
    
    /// Lấy thời gian còn lại trước khi khóa hết hạn (giây)
    pub fn remaining_time(&self) -> i64 {
        let now = Instant::now();
        if now > self.expiry {
            return 0;
        }
        (self.expiry - now).as_secs() as i64
    }
}

/// Giao diện cho vault lưu trữ khóa an toàn
#[async_trait::async_trait]
pub trait KeyVault: Send + Sync + 'static {
    /// Lấy khóa từ vault
    async fn get_key(&self, key_id: &str) -> KeyResult<SecurePrivateKey>;
    
    /// Lưu khóa vào vault
    async fn store_key(&self, key_id: &str, key: &str) -> KeyResult<()>;
    
    /// Xóa khóa khỏi vault
    async fn delete_key(&self, key_id: &str) -> KeyResult<()>;
    
    /// Kiểm tra xem khóa có tồn tại trong vault không
    async fn has_key(&self, key_id: &str) -> KeyResult<bool>;
    
    /// Liệt kê tất cả ID khóa (không bao gồm khóa)
    async fn list_keys(&self) -> KeyResult<Vec<String>>;
    
    /// Gia hạn khóa
    async fn renew_key(&self, key_id: &str) -> KeyResult<()>;
    
    /// Lấy thông tin trạng thái của vault
    async fn status(&self) -> KeyResult<KeyVaultStatus>;
}

/// Thông tin trạng thái của vault
#[derive(Debug, Clone)]
pub struct KeyVaultStatus {
    /// Số lượng khóa trong vault
    pub key_count: usize,
    /// Vault có hoạt động bình thường không
    pub is_healthy: bool,
    /// Thông tin lỗi nếu có
    pub error_info: Option<String>,
}

/// Cài đặt mặc định cho vault
pub struct MemoryKeyVault {
    /// Lưu trữ khóa trong bộ nhớ (trong thực tế nên sử dụng giải pháp an toàn hơn)
    keys: RwLock<HashMap<String, SecurePrivateKey>>,
    /// Thời điểm khởi tạo vault
    created_at: Instant,
    /// Lịch sử truy cập khóa
    access_log: RwLock<Vec<KeyAccessLog>>,
}

/// Thông tin log truy cập khóa
#[derive(Debug, Clone)]
struct KeyAccessLog {
    /// ID của khóa
    key_id: String,
    /// Loại thao tác (get, store, delete)
    operation: String,
    /// Thời điểm
    timestamp: Instant,
    /// Kết quả (success/error)
    result: String,
}

impl MemoryKeyVault {
    /// Tạo vault mới trong bộ nhớ
    pub fn new() -> Self {
        Self {
            keys: RwLock::new(HashMap::new()),
            created_at: Instant::now(),
            access_log: RwLock::new(Vec::new()),
        }
    }
    
    /// Ghi log truy cập khóa
    async fn log_access(&self, key_id: &str, operation: &str, result: &str) {
        let log_entry = KeyAccessLog {
            key_id: key_id.to_string(),
            operation: operation.to_string(),
            timestamp: Instant::now(),
            result: result.to_string(),
        };
        
        let mut log = self.access_log.write().await;
        log.push(log_entry);
        
        // Chỉ giữ tối đa 1000 log gần nhất
        if log.len() > 1000 {
            log.remove(0);
        }
    }
    
    /// Dọn dẹp các khóa hết hạn
    async fn cleanup_expired_keys(&self) -> usize {
        let mut keys = self.keys.write().await;
        let before = keys.len();
        
        // Lọc ra các khóa còn hiệu lực
        keys.retain(|_, key| key.is_valid());
        
        let removed = before - keys.len();
        if removed > 0 {
            debug!("Đã dọn dẹp {} khóa hết hạn", removed);
        }
        
        removed
    }
}

#[async_trait::async_trait]
impl KeyVault for MemoryKeyVault {
    async fn get_key(&self, key_id: &str) -> KeyResult<SecurePrivateKey> {
        // Dọn dẹp các khóa hết hạn
        self.cleanup_expired_keys().await;
        
        let keys = self.keys.read().await;
        match keys.get(key_id) {
            Some(key) => {
                // Kiểm tra tính hợp lệ
                if !key.is_valid() {
                    self.log_access(key_id, "get", "error:expired").await;
                    return Err(KeyError::KeyExpired(key_id.to_string()));
                }
                
                self.log_access(key_id, "get", "success").await;
                Ok(key.clone())
            },
            None => {
                self.log_access(key_id, "get", "error:not_found").await;
                Err(KeyError::KeyNotFound(key_id.to_string()))
            }
        }
    }
    
    async fn store_key(&self, key_id: &str, key: &str) -> KeyResult<()> {
        // Dọn dẹp các khóa hết hạn
        self.cleanup_expired_keys().await;
        
        let mut keys = self.keys.write().await;
        
        // Kiểm tra xem khóa đã tồn tại chưa
        if keys.contains_key(key_id) {
            self.log_access(key_id, "store", "error:already_exists").await;
            return Err(KeyError::KeyAlreadyExists(key_id.to_string()));
        }
        
        keys.insert(key_id.to_string(), SecurePrivateKey::new(key_id, key));
        self.log_access(key_id, "store", "success").await;
        Ok(())
    }
    
    async fn delete_key(&self, key_id: &str) -> KeyResult<()> {
        let mut keys = self.keys.write().await;
        if keys.remove(key_id).is_none() {
            self.log_access(key_id, "delete", "error:not_found").await;
            return Err(KeyError::KeyNotFound(key_id.to_string()));
        }
        
        self.log_access(key_id, "delete", "success").await;
        Ok(())
    }
    
    async fn has_key(&self, key_id: &str) -> KeyResult<bool> {
        // Dọn dẹp các khóa hết hạn
        self.cleanup_expired_keys().await;
        
        let keys = self.keys.read().await;
        let has_key = keys.contains_key(key_id);
        
        self.log_access(key_id, "check", if has_key { "exists" } else { "not_found" }).await;
        Ok(has_key)
    }
    
    async fn list_keys(&self) -> KeyResult<Vec<String>> {
        // Dọn dẹp các khóa hết hạn
        self.cleanup_expired_keys().await;
        
        let keys = self.keys.read().await;
        let key_ids: Vec<String> = keys.keys().cloned().collect();
        
        self.log_access("all", "list", &format!("count:{}", key_ids.len())).await;
        Ok(key_ids)
    }
    
    async fn renew_key(&self, key_id: &str) -> KeyResult<()> {
        let mut keys = self.keys.write().await;
        
        if let Some(key) = keys.get_mut(key_id) {
            key.renew();
            self.log_access(key_id, "renew", "success").await;
            Ok(())
        } else {
            self.log_access(key_id, "renew", "error:not_found").await;
            Err(KeyError::KeyNotFound(key_id.to_string()))
        }
    }
    
    async fn status(&self) -> KeyResult<KeyVaultStatus> {
        // Dọn dẹp các khóa hết hạn
        self.cleanup_expired_keys().await;
        
        let keys = self.keys.read().await;
        let status = KeyVaultStatus {
            key_count: keys.len(),
            is_healthy: true,
            error_info: None,
        };
        
        Ok(status)
    }
}

/// Thông tin cấu hình bridge với bảo mật nâng cao
#[derive(Clone, Serialize, Deserialize)]
pub struct BridgeConfig {
    /// Chain sử dụng làm hub
    pub hub_chain: DmdChain,
    /// Địa chỉ hợp đồng bridge trên hub
    pub hub_contract_address: String,
    /// Địa chỉ ví của bridge operator
    pub operator_address: String,
    /// ID của khóa riêng tư (thay vì lưu trực tiếp)
    #[serde(skip_serializing)]
    operator_key_id: Option<String>,
    /// Thời gian tối đa chờ xác nhận (tính bằng giây)
    pub confirmation_timeout: u64,
    /// Số block cần để xác nhận trên mỗi chain
    pub confirmation_blocks: HashMap<DmdChain, u64>,
    /// Phí bridge theo phần trăm
    pub fee_percentage: f64,
    /// Phí bridge tối thiểu
    pub min_fee: HashMap<DmdChain, String>,
    /// Địa chỉ của hợp đồng ERC20 trên các chain EVM
    pub erc20_addresses: HashMap<DmdChain, String>,
    /// Địa chỉ của hợp đồng ERC1155 trên các chain EVM
    pub erc1155_addresses: HashMap<DmdChain, String>,
    /// Vault cho lưu trữ khóa (không serialize)
    #[serde(skip)]
    key_vault: Option<Arc<dyn KeyVault>>,
}

impl BridgeConfig {
    /// Tạo mới cấu hình bridge
    pub fn new(
        hub_chain: DmdChain,
        hub_contract_address: String,
        operator_address: String,
        operator_key_id: Option<String>,
        confirmation_timeout: u64,
        confirmation_blocks: HashMap<DmdChain, u64>,
        fee_percentage: f64,
        min_fee: HashMap<DmdChain, String>,
        erc20_addresses: HashMap<DmdChain, String>,
        erc1155_addresses: HashMap<DmdChain, String>,
    ) -> Self {
        Self {
            hub_chain,
            hub_contract_address,
            operator_address,
            operator_key_id,
            confirmation_timeout,
            confirmation_blocks,
            fee_percentage,
            min_fee,
            erc20_addresses,
            erc1155_addresses,
            key_vault: None,
        }
    }
    
    /// Thiết lập vault cho lưu trữ khóa
    pub fn with_key_vault(mut self, vault: Arc<dyn KeyVault>) -> Self {
        self.key_vault = Some(vault);
        self
    }
    
    /// Kiểm tra xem có khóa riêng tư hay không
    pub fn has_operator_key(&self) -> bool {
        self.operator_key_id.is_some()
    }
    
    /// Sử dụng khóa riêng tư để ký giao dịch, với các biện pháp bảo mật
    pub async fn get_operator_key_for_signing(&self) -> KeyResult<SecurePrivateKey> {
        // Kiểm tra xem có ID của khóa không
        let key_id = self.operator_key_id.as_ref()
            .ok_or_else(|| KeyError::KeyNotFound("Không có ID khóa riêng tư".to_string()))?;
            
        // Kiểm tra xem có vault không
        let vault = self.key_vault.as_ref()
            .ok_or_else(|| KeyError::VaultNotInitialized)?;
        
        // Kiểm tra trạng thái vault trước khi truy cập khóa
        match vault.status().await {
            Ok(status) if !status.is_healthy => {
                let error_msg = status.error_info.unwrap_or_else(|| "Unknown error".to_string());
                error!("Key vault không khả dụng: {}", error_msg);
                return Err(KeyError::VaultAccessError(error_msg));
            },
            Err(e) => {
                error!("Không thể kiểm tra trạng thái vault: {}", e);
                return Err(KeyError::VaultAccessError(e.to_string()));
            },
            _ => {} // Vault hoạt động bình thường
        }
            
        // Lấy khóa từ vault
        match vault.get_key(key_id).await {
            Ok(key) => {
                if !key.is_valid() {
                    error!("Khóa hết hạn hoặc vượt quá số lần truy cập: {}", key_id);
                    return Err(KeyError::KeyExpired(key_id.clone()));
                }
                Ok(key)
            },
            Err(e) => {
                error!("Không thể lấy khóa {}: {}", key_id, e);
                Err(e)
            }
        }
    }
    
    /// Cố gắng gia hạn khóa hiện tại của operator
    pub async fn try_renew_operator_key(&self) -> KeyResult<bool> {
        if let Some(key_id) = &self.operator_key_id {
            if let Some(vault) = &self.key_vault {
                match vault.renew_key(key_id).await {
                    Ok(_) => {
                        info!("Đã gia hạn khóa operator thành công: {}", key_id);
                        return Ok(true);
                    },
                    Err(e) => {
                        warn!("Không thể gia hạn khóa operator {}: {}", key_id, e);
                        return Err(e);
                    }
                }
            }
        }
        
        Ok(false) // Không có khóa để gia hạn
    }
    
    /// Thiết lập ID khóa riêng tư mới
    pub fn set_operator_key_id(&mut self, key_id: Option<String>) {
        self.operator_key_id = key_id;
    }
    
    /// Lưu khóa riêng tư vào vault
    pub async fn store_operator_key(&self, private_key: &str) -> KeyResult<String> {
        // Kiểm tra xem có vault không
        let vault = self.key_vault.as_ref()
            .ok_or_else(|| KeyError::VaultNotInitialized)?;
            
        // Kiểm tra trạng thái vault trước khi lưu khóa
        match vault.status().await {
            Ok(status) if !status.is_healthy => {
                let error_msg = status.error_info.unwrap_or_else(|| "Unknown error".to_string());
                error!("Key vault không khả dụng: {}", error_msg);
                return Err(KeyError::VaultAccessError(error_msg));
            },
            Err(e) => {
                error!("Không thể kiểm tra trạng thái vault: {}", e);
                return Err(KeyError::VaultAccessError(e.to_string()));
            },
            _ => {} // Vault hoạt động bình thường
        }
            
        // Tạo ID mới cho khóa
        let key_id = format!("operator_key_{}", uuid::Uuid::new_v4());
        
        // Lưu khóa vào vault
        match vault.store_key(&key_id, private_key).await {
            Ok(_) => {
                info!("Đã lưu khóa operator mới: {}", key_id);
                Ok(key_id)
            },
            Err(e) => {
                error!("Không thể lưu khóa operator: {}", e);
                Err(e)
            }
        }
    }
    
    /// Xóa khóa riêng tư khỏi vault
    pub async fn remove_operator_key(&self) -> KeyResult<()> {
        // Kiểm tra xem có ID của khóa không
        let key_id = self.operator_key_id.as_ref()
            .ok_or_else(|| KeyError::KeyNotFound("Không có ID khóa riêng tư".to_string()))?;
            
        // Kiểm tra xem có vault không
        let vault = self.key_vault.as_ref()
            .ok_or_else(|| KeyError::VaultNotInitialized)?;
            
        // Xóa khóa từ vault
        match vault.delete_key(key_id).await {
            Ok(_) => {
                info!("Đã xóa khóa operator: {}", key_id);
                Ok(())
            },
            Err(e) => {
                error!("Không thể xóa khóa operator {}: {}", key_id, e);
                Err(e)
            }
        }
    }
    
    /// Kiểm tra trạng thái vault
    pub async fn check_vault_status(&self) -> KeyResult<KeyVaultStatus> {
        let vault = self.key_vault.as_ref()
            .ok_or_else(|| KeyError::VaultNotInitialized)?;
            
        vault.status().await
    }
}

/// Tạo bridge config với vault mặc định
pub fn create_default_bridge_config() -> BridgeConfig {
    let mut default_confirmation_blocks = HashMap::new();
    default_confirmation_blocks.insert(DmdChain::Ethereum, 12);
    default_confirmation_blocks.insert(DmdChain::BinanceSmartChain, 15);
    default_confirmation_blocks.insert(DmdChain::Polygon, 80);
    default_confirmation_blocks.insert(DmdChain::Near, 2);
    
    let mut default_min_fee = HashMap::new();
    default_min_fee.insert(DmdChain::Ethereum, "0.001".to_string());
    default_min_fee.insert(DmdChain::BinanceSmartChain, "0.0005".to_string());
    default_min_fee.insert(DmdChain::Polygon, "0.0001".to_string());
    default_min_fee.insert(DmdChain::Near, "0.0005".to_string());
    
    let mut default_erc20_addresses = HashMap::new();
    default_erc20_addresses.insert(DmdChain::Ethereum, "0x4A1f9D5182A3bA8218C30Ca30Eb9cf99F50e46eA".to_string());
    default_erc20_addresses.insert(DmdChain::BinanceSmartChain, "0x7B2c5b190C85b4a21A0F4A94Cff883bBc0594444".to_string());
    default_erc20_addresses.insert(DmdChain::Polygon, "0x19A8Ed4860007A66805782Ed7E0d4a7AD11c1c3A".to_string());
    
    let mut default_erc1155_addresses = HashMap::new();
    default_erc1155_addresses.insert(DmdChain::Ethereum, "0x90fE084F877C65e1b577c7b2eA64B8D8dd1AB278".to_string());
    default_erc1155_addresses.insert(DmdChain::BinanceSmartChain, "0x16a7FA783c2FF584B234e13825960F95244Fc7bC".to_string());
    default_erc1155_addresses.insert(DmdChain::Polygon, "0x6B175474E89094C44Da98b954EedeAC495271d0F".to_string());
    
    BridgeConfig::new(
        DmdChain::Near,
        "dmd_bridge.near".to_string(),
        "operator.near".to_string(),
        None,
        600, // 10 phút timeout
        default_confirmation_blocks,
        0.25, // 0.25% phí
        default_min_fee,
        default_erc20_addresses,
        default_erc1155_addresses,
    ).with_key_vault(Arc::new(MemoryKeyVault::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_secure_private_key() {
        let mut key = SecurePrivateKey::new("test_private_key");
        assert!(key.is_valid());
        
        // Truy cập khóa
        let exposed = key.expose_for_signing().unwrap();
        assert_eq!(exposed, "test_private_key");
        assert_eq!(key.access_count, 1);
        
        // Gia hạn khóa
        key.renew();
        assert!(key.is_valid());
        
        // Xóa khóa
        key.clear();
    }
    
    #[tokio::test]
    async fn test_memory_key_vault() {
        let vault = MemoryKeyVault::new();
        
        // Lưu khóa
        vault.store_key("test_key", "test_private_key").await.unwrap();
        
        // Kiểm tra khóa tồn tại
        assert!(vault.has_key("test_key").await.unwrap());
        
        // Lấy khóa
        let key = vault.get_key("test_key").await.unwrap();
        assert!(key.is_valid());
        
        // Xóa khóa
        vault.delete_key("test_key").await.unwrap();
        assert!(!vault.has_key("test_key").await.unwrap());
    }
    
    #[tokio::test]
    async fn test_bridge_config_with_vault() {
        let vault = Arc::new(MemoryKeyVault::new());
        let mut config = create_default_bridge_config();
        config.key_vault = Some(vault.clone());
        
        // Lưu khóa
        let key_id = config.store_operator_key("test_private_key").await.unwrap();
        config.set_operator_key_id(Some(key_id));
        
        // Kiểm tra có khóa
        assert!(config.has_operator_key());
        
        // Lấy khóa để ký
        let key = config.get_operator_key_for_signing().await.unwrap();
        assert!(key.is_valid());
        
        // Xóa khóa
        config.remove_operator_key().await.unwrap();
    }
} 