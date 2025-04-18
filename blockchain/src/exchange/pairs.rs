//! Module quản lý token pairs và liquidity pools
//!
//! Module này cung cấp các chức năng để quản lý token pairs và liquidity pools trên DEX.
//! Hiện tại đang trong giai đoạn phát triển ban đầu.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use log::{info, debug, warn, error};
use anyhow::{Result, Context};
use std::fs;
use std::path::{Path, PathBuf};
use std::io::{self, Read, Write};
use tokio::sync::RwLock;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Lỗi có thể xảy ra trong quá trình quản lý pairs
#[derive(Error, Debug)]
pub enum PairError {
    #[error("Pair không tồn tại: {0}")]
    PairNotFound(String),
    
    #[error("Token không tồn tại: {0}")]
    TokenNotFound(String),
    
    #[error("Liquidity không đủ: yêu cầu {required}, hiện có {available}")]
    InsufficientLiquidity {
        required: f64,
        available: f64,
    },
    
    #[error("Slippage vượt quá giới hạn: {actual}% > {max}%")]
    SlippageExceeded {
        actual: f64,
        max: f64,
    },
    
    #[error("Lỗi hệ thống: {0}")]
    SystemError(String),
    
    #[error("Giá trị không hợp lệ: {0}")]
    InvalidValue(String),
    
    #[error("Yêu cầu xác thực: {0}")]
    ValidationError(String),
    
    #[error("Conflict: {0}")]
    Conflict(String),
}

/// Kết quả của các hoạt động pair
pub type PairResult<T> = Result<T, PairError>;

/// Error types for exchange operations
#[derive(Error, Debug)]
pub enum ExchangeError {
    #[error("Pair không tồn tại: {0}")]
    PairNotFound(String),
    
    #[error("Token không tồn tại: {0}")]
    TokenNotFound(String),
    
    #[error("Tham số không hợp lệ: {0}")]
    InvalidParameter(String),
    
    #[error("Lỗi hệ thống: {0}")]
    SystemError(String),
}

/// Interface cho lưu trữ bền vững các TokenPair
#[async_trait::async_trait]
pub trait PairRepository: Send + Sync {
    /// Lưu một TokenPair
    async fn save_pair(&self, pair: &TokenPair) -> Result<(), String>;
    
    /// Lưu nhiều TokenPair
    async fn save_pairs(&self, pairs: &[TokenPair]) -> Result<(), String>;
    
    /// Tìm kiếm TokenPair theo ID
    async fn find_by_id(&self, id: &str) -> Result<Option<TokenPair>, String>;
    
    /// Lấy tất cả TokenPair
    async fn get_all_pairs(&self) -> Result<Vec<TokenPair>, String>;
    
    /// Xóa một TokenPair
    async fn delete_pair(&self, id: &str) -> Result<bool, String>;
    
    /// Đồng bộ dữ liệu vào storage
    async fn flush(&self) -> Result<(), String>;
}

/// Cài đặt repository lưu trữ JSON cho TokenPair
pub struct JsonPairRepository {
    /// Đường dẫn file lưu trữ
    file_path: PathBuf,
    
    /// Cache trong bộ nhớ
    pairs: RwLock<HashMap<String, TokenPair>>,
}

impl JsonPairRepository {
    /// Tạo mới repository
    pub async fn new(file_path: impl AsRef<Path>) -> Result<Self, String> {
        let file_path = file_path.as_ref().to_path_buf();
        let pairs = RwLock::new(HashMap::new());
        
        // Đảm bảo thư mục cha tồn tại
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Không thể tạo thư mục cha: {}", e))?;
        }
        
        // Nếu file tồn tại, đọc dữ liệu
        if file_path.exists() {
            let data = fs::read_to_string(&file_path)
                .map_err(|e| format!("Không thể đọc file: {}", e))?;
            
            if !data.trim().is_empty() {
                let loaded_pairs: HashMap<String, TokenPair> = serde_json::from_str(&data)
                    .map_err(|e| format!("Không thể parse JSON: {}", e))?;
                
                let mut pairs_guard = pairs.write().await;
                *pairs_guard = loaded_pairs;
                
                info!("Đã tải {} token pairs từ {}", pairs_guard.len(), file_path.display());
            }
        } else {
            // Tạo file mới nếu chưa tồn tại
            fs::write(&file_path, "{}")
                .map_err(|e| format!("Không thể tạo file: {}", e))?;
            
            info!("Đã tạo file lưu trữ mới tại {}", file_path.display());
        }
        
        Ok(Self {
            file_path,
            pairs,
        })
    }
    
    /// Lưu dữ liệu vào file
    async fn save_to_file(&self) -> Result<(), String> {
        let pairs_guard = self.pairs.read().await;
        let json = serde_json::to_string_pretty(&*pairs_guard)
            .map_err(|e| format!("Không thể chuyển đổi sang JSON: {}", e))?;
        
        // Tạo file tạm thời
        let temp_path = self.file_path.with_extension("tmp");
        
        // Sao lưu file hiện tại nếu có
        if self.file_path.exists() {
            let backup_path = self.file_path.with_extension("bak");
            fs::copy(&self.file_path, &backup_path)
                .map_err(|e| format!("Không thể sao lưu file: {}", e))?;
        }
        
        // Ghi vào file tạm
        fs::write(&temp_path, &json)
            .map_err(|e| format!("Không thể ghi vào file tạm: {}", e))?;
        
        // Đổi tên file tạm thành file chính
        fs::rename(&temp_path, &self.file_path)
            .map_err(|e| format!("Không thể đổi tên file tạm: {}", e))?;
        
        debug!("Đã lưu {} token pairs vào {}", pairs_guard.len(), self.file_path.display());
        Ok(())
    }
}

#[async_trait::async_trait]
impl PairRepository for JsonPairRepository {
    async fn save_pair(&self, pair: &TokenPair) -> Result<(), String> {
        let mut pairs_guard = self.pairs.write().await;
        pairs_guard.insert(pair.id.clone(), pair.clone());
        
        // Lưu vào file
        drop(pairs_guard); // Giải phóng lock trước khi gọi hàm khác
        self.save_to_file().await
    }
    
    async fn save_pairs(&self, pairs: &[TokenPair]) -> Result<(), String> {
        if pairs.is_empty() {
            return Ok(());
        }
        
        let mut pairs_guard = self.pairs.write().await;
        
        for pair in pairs {
            pairs_guard.insert(pair.id.clone(), pair.clone());
        }
        
        // Lưu vào file
        drop(pairs_guard); // Giải phóng lock trước khi gọi hàm khác
        self.save_to_file().await
    }
    
    async fn find_by_id(&self, id: &str) -> Result<Option<TokenPair>, String> {
        let pairs_guard = self.pairs.read().await;
        Ok(pairs_guard.get(id).cloned())
    }
    
    async fn get_all_pairs(&self) -> Result<Vec<TokenPair>, String> {
        let pairs_guard = self.pairs.read().await;
        Ok(pairs_guard.values().cloned().collect())
    }
    
    async fn delete_pair(&self, id: &str) -> Result<bool, String> {
        let mut pairs_guard = self.pairs.write().await;
        let existed = pairs_guard.remove(id).is_some();
        
        if existed {
            // Lưu vào file
            drop(pairs_guard); // Giải phóng lock trước khi gọi hàm khác
            self.save_to_file().await?;
        }
        
        Ok(existed)
    }
    
    async fn flush(&self) -> Result<(), String> {
        self.save_to_file().await
    }
}

/// Thông tin về token pair
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenPair {
    /// ID của pair
    pub id: String,
    
    /// Địa chỉ của token 1
    pub token1_address: String,
    
    /// Địa chỉ của token 2
    pub token2_address: String,
    
    /// Tên của pair (VD: ETH-USDT)
    pub name: String,
    
    /// Tỷ lệ token 1 / token 2
    pub exchange_rate: f64,
    
    /// Tổng liquidity của token 1
    pub token1_liquidity: f64,
    
    /// Tổng liquidity của token 2
    pub token2_liquidity: f64,
    
    /// Địa chỉ của pool contract
    pub pool_address: Option<String>,
    
    /// Thời gian tạo pair
    pub created_at: u64,
    
    /// Thời gian cập nhật cuối cùng
    pub updated_at: u64,
    
    /// Phiên bản của dữ liệu, dùng cho xác thực optimistic locking
    #[serde(default)]
    pub version: u64,
}

impl TokenPair {
    /// Xác thực tính hợp lệ của dữ liệu TokenPair
    pub fn validate(&self) -> PairResult<()> {
        // Kiểm tra địa chỉ token 1
        if self.token1_address.trim().is_empty() {
            return Err(PairError::ValidationError("Địa chỉ token 1 không được để trống".to_string()));
        }
        
        // Kiểm tra địa chỉ token 2
        if self.token2_address.trim().is_empty() {
            return Err(PairError::ValidationError("Địa chỉ token 2 không được để trống".to_string()));
        }
        
        // Đảm bảo hai token khác nhau
        if self.token1_address == self.token2_address {
            return Err(PairError::ValidationError("Hai token phải khác nhau".to_string()));
        }
        
        // Kiểm tra tên pair
        if self.name.trim().is_empty() {
            return Err(PairError::ValidationError("Tên pair không được để trống".to_string()));
        }
        
        // Kiểm tra giá trị liquidity không âm
        if self.token1_liquidity < 0.0 {
            return Err(PairError::ValidationError("Liquidity của token 1 không được âm".to_string()));
        }
        
        if self.token2_liquidity < 0.0 {
            return Err(PairError::ValidationError("Liquidity của token 2 không được âm".to_string()));
        }
        
        // Kiểm tra tỷ lệ trao đổi
        if self.exchange_rate <= 0.0 {
            return Err(PairError::ValidationError("Tỷ lệ trao đổi phải lớn hơn 0".to_string()));
        }
        
        // Kiểm tra timestamp
        if self.created_at == 0 {
            return Err(PairError::ValidationError("Thời gian tạo không hợp lệ".to_string()));
        }
        
        if self.updated_at < self.created_at {
            return Err(PairError::ValidationError("Thời gian cập nhật không thể nhỏ hơn thời gian tạo".to_string()));
        }
        
        // Kiểm tra định dạng địa chỉ pool contract nếu có
        if let Some(address) = &self.pool_address {
            if address.trim().is_empty() {
                return Err(PairError::ValidationError("Địa chỉ pool không được để trống nếu được cung cấp".to_string()));
            }
            
            // Kiểm tra địa chỉ pool có phải là địa chỉ Ethereum hợp lệ
            if !address.starts_with("0x") || address.len() != 42 || !address[2..].chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(PairError::ValidationError("Địa chỉ pool không hợp lệ, cần đúng định dạng Ethereum (0x...)".to_string()));
            }
        }
        
        Ok(())
    }
    
    /// Kiểm tra định dạng địa chỉ token
    pub fn validate_token_address(address: &str) -> PairResult<()> {
        if address.trim().is_empty() {
            return Err(PairError::ValidationError("Địa chỉ token không được để trống".to_string()));
        }
        
        // Kiểm tra định dạng địa chỉ Ethereum
        if address.starts_with("0x") {
            if address.len() != 42 || !address[2..].chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(PairError::ValidationError("Địa chỉ token không hợp lệ, phải đúng định dạng Ethereum (0x...)".to_string()));
            }
        } else {
            return Err(PairError::ValidationError("Địa chỉ token phải bắt đầu bằng 0x".to_string()));
        }
        
        Ok(())
    }
}

/// Quản lý token pairs với lưu trữ bền vững
#[derive(Debug)]
pub struct PairManager {
    /// Các token pairs, key là pair ID
    pairs: HashMap<String, TokenPair>,
    
    /// ID counter cho pairs
    pair_id_counter: u64,
    
    /// Repository để lưu trữ bền vững
    repository: Option<Arc<dyn PairRepository>>,
}

impl PairManager {
    /// Tạo một PairManager mới
    pub fn new() -> Self {
        Self {
            pairs: HashMap::new(),
            pair_id_counter: 0,
            repository: None,
        }
    }
    
    /// Thiết lập repository lưu trữ
    pub fn with_repository(mut self, repository: Arc<dyn PairRepository>) -> Self {
        self.repository = Some(repository);
        self
    }
    
    /// Tạo một token pair mới với xác thực dữ liệu
    pub async fn create_pair(&mut self, token1_address: &str, token2_address: &str, name: &str) -> PairResult<String> {
        // Xác thực địa chỉ token
        TokenPair::validate_token_address(token1_address)?;
        TokenPair::validate_token_address(token2_address)?;
        
        // Kiểm tra token 1 và token 2 khác nhau
        if token1_address == token2_address {
            return Err(PairError::ValidationError("Hai token phải khác nhau".to_string()));
        }
        
        // Kiểm tra tên pair
        if name.trim().is_empty() {
            return Err(PairError::ValidationError("Tên pair không được để trống".to_string()));
        }
        
        // Kiểm tra xem pair đã tồn tại chưa
        let existing_pair = self.pairs.values().find(|p| 
            (p.token1_address == token1_address && p.token2_address == token2_address) ||
            (p.token1_address == token2_address && p.token2_address == token1_address)
        );
        
        if let Some(pair) = existing_pair {
            return Err(PairError::Conflict(format!(
                "Pair cho tokens {} và {} đã tồn tại với ID: {}", 
                token1_address, token2_address, pair.id
            )));
        }
        
        let now = get_current_timestamp();
        let id = format!("pair_{}", self.pair_id_counter);
        self.pair_id_counter += 1;
        
        let pair = TokenPair {
            id: id.clone(),
            token1_address: token1_address.to_string(),
            token2_address: token2_address.to_string(),
            name: name.to_string(),
            exchange_rate: 1.0, // Tỷ lệ mặc định
            token1_liquidity: 0.0,
            token2_liquidity: 0.0,
            pool_address: None,
            created_at: now,
            updated_at: now,
            version: 1, // Phiên bản đầu tiên
        };
        
        // Xác thực lại toàn bộ dữ liệu
        pair.validate()?;
        
        self.pairs.insert(id.clone(), pair.clone());
        
        // Lưu vào repository nếu có
        if let Some(repo) = &self.repository {
            match repo.save_pair(&pair).await {
                Ok(_) => {
                    debug!("Đã lưu token pair mới vào repository: {}", id);
                },
                Err(e) => {
                    warn!("Không thể lưu token pair vào repository: {}", e);
                }
            }
        }
        
        info!("Token pair mới được tạo: {} ({}-{})", id, token1_address, token2_address);
        Ok(id)
    }
    
    /// Khởi tạo từ repository
    pub async fn load_from_repository(&mut self) -> PairResult<usize> {
        if let Some(repo) = &self.repository {
            match repo.get_all_pairs().await {
                Ok(pairs) => {
                    // Cập nhật counter dựa trên ID hiện có
                    for pair in &pairs {
                        if let Some(id_str) = pair.id.strip_prefix("pair_") {
                            if let Ok(id) = id_str.parse::<u64>() {
                                if id >= self.pair_id_counter {
                                    self.pair_id_counter = id + 1;
                                }
                            }
                        }
                    }
                    
                    // Cập nhật HashMap
                    for pair in pairs.iter() {
                        self.pairs.insert(pair.id.clone(), pair.clone());
                    }
                    
                    info!("Đã tải {} token pairs từ repository", pairs.len());
                    Ok(pairs.len())
                },
                Err(e) => {
                    error!("Không thể tải token pairs từ repository: {}", e);
                    Err(PairError::SystemError(format!("Không thể tải từ repository: {}", e)))
                }
            }
        } else {
            warn!("Không thể tải token pairs vì chưa thiết lập repository");
            Ok(0)
        }
    }
    
    /// Lấy thông tin về token pair
    pub fn get_pair(&self, pair_id: &str) -> PairResult<&TokenPair> {
        self.pairs.get(pair_id)
            .ok_or_else(|| PairError::PairNotFound(pair_id.to_string()))
    }
    
    /// Thêm liquidity vào pair với xác thực dữ liệu
    pub async fn add_liquidity(&mut self, pair_id: &str, token1_amount: f64, token2_amount: f64) -> PairResult<()> {
        // Kiểm tra giá trị đầu vào
        if token1_amount <= 0.0 || token2_amount <= 0.0 {
            return Err(PairError::InsufficientLiquidity {
                required: token1_amount.max(token2_amount),
                available: 0.0,
            });
        }
        
        // Kiểm tra giới hạn số lượng (để tránh overflow hoặc giá trị cực lớn)
        const MAX_AMOUNT: f64 = 1_000_000_000_000_000.0; // 1 quadrillion
        if token1_amount > MAX_AMOUNT || token2_amount > MAX_AMOUNT {
            return Err(PairError::InvalidValue(format!(
                "Số lượng token quá lớn, giới hạn là {}", MAX_AMOUNT
            )));
        }
        
        let pair = self.pairs.get_mut(pair_id)
            .ok_or_else(|| PairError::PairNotFound(pair_id.to_string()))?;
        
        // Lưu phiên bản hiện tại để kiểm tra optimistic locking
        let current_version = pair.version;
            
        pair.token1_liquidity += token1_amount;
        pair.token2_liquidity += token2_amount;
        
        // Cập nhật tỷ lệ trao đổi nếu có liquidity đầy đủ
        if pair.token1_liquidity > 0.0 && pair.token2_liquidity > 0.0 {
            pair.exchange_rate = pair.token1_liquidity / pair.token2_liquidity;
        }
        
        pair.updated_at = get_current_timestamp();
        pair.version += 1; // Tăng phiên bản
        
        // Xác thực lại dữ liệu pair sau khi thay đổi
        pair.validate()?;
        
        // Lưu vào repository nếu có
        let pair_clone = pair.clone();
        if let Some(repo) = &self.repository {
            // Kiểm tra xem pair trong repository có bị thay đổi không (optimistic locking)
            if let Ok(Some(repo_pair)) = repo.find_by_id(pair_id).await {
                if repo_pair.version != current_version {
                    // Phiên bản không khớp, đã có thay đổi từ nguồn khác
                    // Khôi phục lại giá trị và báo lỗi
                    *pair = repo_pair;
                    return Err(PairError::Conflict(
                        "Pair đã được cập nhật bởi một phiên làm việc khác, vui lòng thử lại".to_string()
                    ));
                }
            }
            
            if let Err(e) = repo.save_pair(&pair_clone).await {
                warn!("Không thể cập nhật token pair trong repository: {}", e);
            }
        }
        
        info!("Đã thêm liquidity ({}, {}) vào pair {}", token1_amount, token2_amount, pair_id);
        Ok(())
    }
    
    /// Rút liquidity từ pair với xác thực dữ liệu
    pub async fn remove_liquidity(&mut self, pair_id: &str, token1_amount: f64, token2_amount: f64) -> PairResult<()> {
        // Kiểm tra giá trị đầu vào
        if token1_amount < 0.0 || token2_amount < 0.0 {
            return Err(PairError::InvalidValue("Số lượng token không được âm".to_string()));
        }
        
        let pair = self.pairs.get_mut(pair_id)
            .ok_or_else(|| PairError::PairNotFound(pair_id.to_string()))?;
        
        // Lưu phiên bản hiện tại để kiểm tra optimistic locking
        let current_version = pair.version;
            
        if token1_amount > pair.token1_liquidity {
            return Err(PairError::InsufficientLiquidity {
                required: token1_amount,
                available: pair.token1_liquidity,
            });
        }
        
        if token2_amount > pair.token2_liquidity {
            return Err(PairError::InsufficientLiquidity {
                required: token2_amount,
                available: pair.token2_liquidity,
            });
        }
        
        // Kiểm tra tỷ lệ rút không làm mất cân bằng pool quá nhiều
        if pair.token1_liquidity > 0.0 && pair.token2_liquidity > 0.0 {
            let current_ratio = pair.token1_liquidity / pair.token2_liquidity;
            let removal_ratio = token1_amount / token2_amount;
            
            // Nếu tỷ lệ rút chênh lệch quá nhiều so với tỷ lệ hiện tại (> 10%)
            const MAX_RATIO_DIFF: f64 = 0.1; // 10%
            
            if (removal_ratio / current_ratio - 1.0).abs() > MAX_RATIO_DIFF {
                return Err(PairError::ValidationError(format!(
                    "Tỷ lệ rút quá chênh lệch so với tỷ lệ hiện tại. Hiện tại: {:.4}, Rút: {:.4}",
                    current_ratio, removal_ratio
                )));
            }
        }
        
        pair.token1_liquidity -= token1_amount;
        pair.token2_liquidity -= token2_amount;
        
        // Cập nhật tỷ lệ trao đổi nếu còn liquidity
        if pair.token1_liquidity > 0.0 && pair.token2_liquidity > 0.0 {
            pair.exchange_rate = pair.token1_liquidity / pair.token2_liquidity;
        }
        
        pair.updated_at = get_current_timestamp();
        pair.version += 1; // Tăng phiên bản
        
        // Xác thực lại dữ liệu pair sau khi thay đổi
        pair.validate()?;
        
        // Lưu vào repository nếu có
        let pair_clone = pair.clone();
        if let Some(repo) = &self.repository {
            // Kiểm tra xem pair trong repository có bị thay đổi không (optimistic locking)
            if let Ok(Some(repo_pair)) = repo.find_by_id(pair_id).await {
                if repo_pair.version != current_version {
                    // Phiên bản không khớp, đã có thay đổi từ nguồn khác
                    // Khôi phục lại giá trị và báo lỗi
                    *pair = repo_pair;
                    return Err(PairError::Conflict(
                        "Pair đã được cập nhật bởi một phiên làm việc khác, vui lòng thử lại".to_string()
                    ));
                }
            }
            
            if let Err(e) = repo.save_pair(&pair_clone).await {
                warn!("Không thể cập nhật token pair trong repository: {}", e);
            }
        }
        
        info!("Đã rút liquidity ({}, {}) từ pair {}", token1_amount, token2_amount, pair_id);
        Ok(())
    }
    
    /// Swap token 1 sang token 2 với xác thực nâng cao
    pub async fn swap_token1_to_token2(&mut self, pair_id: &str, token1_amount: f64, max_slippage: f64) -> PairResult<f64> {
        // Kiểm tra giá trị đầu vào
        if token1_amount <= 0.0 {
            return Err(PairError::InvalidValue("Số lượng token phải lớn hơn 0".to_string()));
        }
        
        // Kiểm tra slippage hợp lệ
        if max_slippage < 0.0 || max_slippage > 100.0 {
            return Err(PairError::InvalidValue("Slippage phải nằm trong khoảng 0-100%".to_string()));
        }
        
        let pair = self.pairs.get_mut(pair_id)
            .ok_or_else(|| PairError::PairNotFound(pair_id.to_string()))?;
        
        // Lưu phiên bản hiện tại để kiểm tra optimistic locking
        let current_version = pair.version;
            
        // Kiểm tra liquidity đủ
        if pair.token1_liquidity == 0.0 || pair.token2_liquidity == 0.0 {
            return Err(PairError::InsufficientLiquidity {
                required: token1_amount,
                available: 0.0,
            });
        }
        
        // Giới hạn kích thước giao dịch để tránh thao túng giá
        let max_tx_size = pair.token1_liquidity * 0.3; // Tối đa 30% pool
        if token1_amount > max_tx_size {
            return Err(PairError::ValidationError(format!(
                "Kích thước giao dịch quá lớn, tối đa là {:.4} token (30% của pool)",
                max_tx_size
            )));
        }
        
        let token2_out: f64;
        
        if token1_amount > pair.token1_liquidity * 0.1 {
            // Kiểm tra slippage
            let k_before = pair.token1_liquidity * pair.token2_liquidity;
            let new_token1 = pair.token1_liquidity + token1_amount;
            let new_token2 = k_before / new_token1;
            token2_out = pair.token2_liquidity - new_token2;
            
            let expected_out = token1_amount / pair.exchange_rate;
            let slippage = (expected_out - token2_out) / expected_out * 100.0;
            
            if slippage > max_slippage {
                return Err(PairError::SlippageExceeded {
                    actual: slippage,
                    max: max_slippage,
                });
            }
            
            // Thực hiện swap
            pair.token1_liquidity = new_token1;
            pair.token2_liquidity = new_token2;
            pair.exchange_rate = new_token1 / new_token2;
            pair.updated_at = get_current_timestamp();
        } else {
            // Swap nhỏ, không ảnh hưởng nhiều đến giá
            token2_out = token1_amount / pair.exchange_rate;
            
            // Đảm bảo có đủ token2
            if token2_out > pair.token2_liquidity {
                return Err(PairError::InsufficientLiquidity {
                    required: token2_out,
                    available: pair.token2_liquidity,
                });
            }
            
            pair.token1_liquidity += token1_amount;
            pair.token2_liquidity -= token2_out;
            pair.updated_at = get_current_timestamp();
        }
        
        pair.version += 1; // Tăng phiên bản
        
        // Xác thực lại dữ liệu pair sau khi thay đổi
        pair.validate()?;
        
        // Lưu vào repository nếu có
        let pair_clone = pair.clone();
        if let Some(repo) = &self.repository {
            // Kiểm tra xem pair trong repository có bị thay đổi không (optimistic locking)
            if let Ok(Some(repo_pair)) = repo.find_by_id(pair_id).await {
                if repo_pair.version != current_version {
                    // Phiên bản không khớp, đã có thay đổi từ nguồn khác
                    // Khôi phục lại giá trị và báo lỗi
                    *pair = repo_pair;
                    return Err(PairError::Conflict(
                        "Pair đã được cập nhật bởi một phiên làm việc khác, vui lòng thử lại".to_string()
                    ));
                }
            }
            
            if let Err(e) = repo.save_pair(&pair_clone).await {
                warn!("Không thể cập nhật token pair trong repository sau swap: {}", e);
            }
        }
        
        info!("Đã swap {} token1 thành {} token2 trong pair {}", 
            token1_amount, token2_out, pair_id);
            
        Ok(token2_out)
    }
    
    /// Lấy tất cả token pairs
    pub fn get_all_pairs(&self) -> Vec<&TokenPair> {
        self.pairs.values().collect()
    }
    
    /// Tìm pairs theo token
    pub fn find_pairs_by_token(&self, token_address: &str) -> Vec<&TokenPair> {
        self.pairs.values()
            .filter(|pair| pair.token1_address == token_address || pair.token2_address == token_address)
            .collect()
    }
    
    /// Cập nhật địa chỉ pool contract với xác thực nâng cao
    pub async fn set_pool_address(&mut self, pair_id: &str, pool_address: &str) -> PairResult<()> {
        // Xác thực địa chỉ pool
        if pool_address.trim().is_empty() {
            return Err(PairError::ValidationError("Địa chỉ pool không được để trống".to_string()));
        }
        
        // Kiểm tra định dạng địa chỉ Ethereum
        if !pool_address.starts_with("0x") || pool_address.len() != 42 || !pool_address[2..].chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(PairError::ValidationError("Địa chỉ pool không hợp lệ, cần đúng định dạng Ethereum (0x...)".to_string()));
        }
        
        let pair = self.pairs.get_mut(pair_id)
            .ok_or_else(|| PairError::PairNotFound(pair_id.to_string()))?;
        
        // Lưu phiên bản hiện tại để kiểm tra optimistic locking
        let current_version = pair.version;
        
        // Kiểm tra xem pool đã có địa chỉ chưa
        if let Some(existing_address) = &pair.pool_address {
            if existing_address == pool_address {
                // Địa chỉ không thay đổi, không cần cập nhật
                return Ok(());
            }
            
            // Cảnh báo khi thay đổi địa chỉ pool đã tồn tại
            warn!("Thay đổi địa chỉ pool từ {} thành {} cho pair {}", existing_address, pool_address, pair_id);
        }
            
        pair.pool_address = Some(pool_address.to_string());
        pair.updated_at = get_current_timestamp();
        pair.version += 1; // Tăng phiên bản
        
        // Xác thực lại dữ liệu pair sau khi thay đổi
        pair.validate()?;
        
        // Lưu vào repository nếu có
        let pair_clone = pair.clone();
        if let Some(repo) = &self.repository {
            // Kiểm tra xem pair trong repository có bị thay đổi không (optimistic locking)
            if let Ok(Some(repo_pair)) = repo.find_by_id(pair_id).await {
                if repo_pair.version != current_version {
                    // Phiên bản không khớp, đã có thay đổi từ nguồn khác
                    // Khôi phục lại giá trị và báo lỗi
                    *pair = repo_pair;
                    return Err(PairError::Conflict(
                        "Pair đã được cập nhật bởi một phiên làm việc khác, vui lòng thử lại".to_string()
                    ));
                }
            }
            
            if let Err(e) = repo.save_pair(&pair_clone).await {
                warn!("Không thể cập nhật pool address trong repository: {}", e);
            }
        }
        
        info!("Đã thiết lập pool address {} cho pair {}", pool_address, pair_id);
        Ok(())
    }
    
    /// Đồng bộ tất cả dữ liệu với repository
    pub async fn flush(&self) -> PairResult<()> {
        if let Some(repo) = &self.repository {
            // Lấy tất cả pairs
            let pairs: Vec<TokenPair> = self.pairs.values().cloned().collect();
            
            if let Err(e) = repo.save_pairs(&pairs).await {
                error!("Không thể đồng bộ dữ liệu với repository: {}", e);
                return Err(PairError::SystemError(format!("Không thể đồng bộ: {}", e)));
            }
            
            debug!("Đã đồng bộ {} token pairs với repository", pairs.len());
            Ok(())
        } else {
            warn!("Không thể đồng bộ vì chưa thiết lập repository");
            Ok(())
        }
    }

    pub fn update_pair(&mut self, pair_id: &str, update: PairUpdateRequest) -> Result<TradingPair, ExchangeError> {
        // Kiểm tra xem cặp có tồn tại không
        if !self.pairs.contains_key(pair_id) {
            return Err(ExchangeError::PairNotFound(pair_id.to_string()));
        }

        let mut pair = self.pairs.get(pair_id).unwrap().clone();
        let validation = PairValidation::default();

        // Xác thực các giá trị cập nhật
        if let Some(fee_percent) = update.fee_percent {
            if fee_percent < 0.0 || fee_percent > validation.max_fee_percent {
                return Err(ExchangeError::InvalidParameter(
                    format!("fee_percent phải nằm trong khoảng 0.0 đến {}", validation.max_fee_percent)
                ));
            }
            info!("Cập nhật fee_percent cho cặp {}: {} -> {}", pair_id, pair.fee_percent, fee_percent);
            pair.fee_percent = fee_percent;
        }

        if let Some(status) = update.status {
            info!("Cập nhật trạng thái cho cặp {}: {:?} -> {:?}", pair_id, pair.status, status);
            pair.status = status;
        }

        if let Some(price_oracle) = update.price_oracle {
            // Xác thực price_oracle
            if price_oracle <= 0.0 {
                return Err(ExchangeError::InvalidParameter(
                    "price_oracle phải lớn hơn 0.0".to_string(),
                ));
            }
            
            // Kiểm tra độ lệch giá so với giá hiện tại
            let current_price = pair.calculate_price();
            
            if current_price > 0.0 {
                let price_deviation = (price_oracle - current_price).abs() / current_price;
                if price_deviation > validation.price_deviation_threshold {
                    warn!("Cảnh báo: price_oracle ({}) có độ lệch lớn ({:.2}%) so với giá hiện tại ({})",
                           price_oracle, price_deviation * 100.0, current_price);
                    return Err(ExchangeError::InvalidParameter(
                        format!("price_oracle ({}) có độ lệch quá lớn ({:.2}%) so với giá hiện tại ({})",
                               price_oracle, price_deviation * 100.0, current_price)
                    ));
                }
            }
            
            info!("Cập nhật price_oracle cho cặp {}: {} -> {}", pair_id, pair.price_oracle, price_oracle);
            pair.price_oracle = price_oracle;
        }

        if let Some(price_impact_limit) = update.price_impact_limit {
            if price_impact_limit < 0.0 || price_impact_limit > validation.max_price_impact {
                return Err(ExchangeError::InvalidParameter(
                    format!("price_impact_limit phải nằm trong khoảng 0.0 đến {}", validation.max_price_impact)
                ));
            }
            info!("Cập nhật price_impact_limit cho cặp {}: {} -> {}", pair_id, pair.price_impact_limit, price_impact_limit);
            pair.price_impact_limit = price_impact_limit;
        }

        // Nếu cập nhật min_liquidity
        if let Some(min_liquidity) = update.min_liquidity {
            if min_liquidity < validation.min_liquidity {
                return Err(ExchangeError::InvalidParameter(
                    format!("min_liquidity phải lớn hơn hoặc bằng {}", validation.min_liquidity)
                ));
            }
            info!("Cập nhật min_liquidity cho cặp {}: {} -> {}", pair_id, pair.min_liquidity, min_liquidity);
            pair.min_liquidity = min_liquidity;
        }

        // Cập nhật thời gian
        pair.updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Lỗi lấy thời gian")
            .as_secs();
        
        // Lưu cập nhật vào map
        self.pairs.insert(pair_id.to_string(), pair.clone());
        
        // Ghi log cập nhật
        info!("Đã cập nhật cặp {}: {:?}", pair_id, pair);
        
        Ok(pair)
    }
}

/// Lấy timestamp hiện tại dưới dạng giây
fn get_current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(std::time::Duration::from_secs(0))
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_create_and_get_pair() {
        let mut manager = PairManager::new();
        
        let token1 = "0xToken1";
        let token2 = "0xToken2";
        let name = "TOKEN1-TOKEN2";
        
        let pair_id = manager.create_pair(token1, token2, name).unwrap();
        
        let pair = manager.get_pair(&pair_id).unwrap();
        assert_eq!(pair.token1_address, token1);
        assert_eq!(pair.token2_address, token2);
        assert_eq!(pair.name, name);
        assert_eq!(pair.token1_liquidity, 0.0);
        assert_eq!(pair.token2_liquidity, 0.0);
        assert_eq!(pair.exchange_rate, 1.0);
    }
    
    #[test]
    fn test_add_and_remove_liquidity() {
        let mut manager = PairManager::new();
        
        let pair_id = manager.create_pair("0xToken1", "0xToken2", "TOKEN1-TOKEN2").unwrap();
        
        // Thêm liquidity
        manager.add_liquidity(&pair_id, 100.0, 200.0).unwrap();
        
        let pair = manager.get_pair(&pair_id).unwrap();
        assert_eq!(pair.token1_liquidity, 100.0);
        assert_eq!(pair.token2_liquidity, 200.0);
        assert_eq!(pair.exchange_rate, 0.5); // 100/200 = 0.5
        
        // Thêm thêm liquidity
        manager.add_liquidity(&pair_id, 50.0, 100.0).unwrap();
        
        let pair = manager.get_pair(&pair_id).unwrap();
        assert_eq!(pair.token1_liquidity, 150.0);
        assert_eq!(pair.token2_liquidity, 300.0);
        assert_eq!(pair.exchange_rate, 0.5); // 150/300 = 0.5
        
        // Rút liquidity
        manager.remove_liquidity(&pair_id, 50.0, 100.0).unwrap();
        
        let pair = manager.get_pair(&pair_id).unwrap();
        assert_eq!(pair.token1_liquidity, 100.0);
        assert_eq!(pair.token2_liquidity, 200.0);
        assert_eq!(pair.exchange_rate, 0.5); // 100/200 = 0.5
    }
    
    #[test]
    fn test_swap_tokens() {
        let mut manager = PairManager::new();
        
        let pair_id = manager.create_pair("0xToken1", "0xToken2", "TOKEN1-TOKEN2").unwrap();
        
        // Thêm liquidity
        manager.add_liquidity(&pair_id, 1000.0, 2000.0).unwrap();
        
        // Swap nhỏ
        let token2_amount = manager.swap_token1_to_token2(&pair_id, 10.0, 1.0).unwrap();
        assert!(token2_amount > 19.0 && token2_amount < 21.0); // Khoảng 20.0
        
        let pair = manager.get_pair(&pair_id).unwrap();
        assert_eq!(pair.token1_liquidity, 1010.0);
        assert!(pair.token2_liquidity < 2000.0 && pair.token2_liquidity > 1980.0);
    }
    
    #[test]
    fn test_find_pairs_by_token() {
        let mut manager = PairManager::new();
        
        let token1 = "0xToken1";
        let token2 = "0xToken2";
        let token3 = "0xToken3";
        
        manager.create_pair(token1, token2, "TOKEN1-TOKEN2").unwrap();
        manager.create_pair(token1, token3, "TOKEN1-TOKEN3").unwrap();
        manager.create_pair(token2, token3, "TOKEN2-TOKEN3").unwrap();
        
        let pairs = manager.find_pairs_by_token(token1);
        assert_eq!(pairs.len(), 2);
        
        let pairs = manager.find_pairs_by_token(token2);
        assert_eq!(pairs.len(), 2);
        
        let pairs = manager.find_pairs_by_token(token3);
        assert_eq!(pairs.len(), 2);
        
        let pairs = manager.find_pairs_by_token("0xNonExistent");
        assert_eq!(pairs.len(), 0);
    }
    
    #[test]
    fn test_error_handling() {
        let mut manager = PairManager::new();
        
        // Pair không tồn tại
        let result = manager.get_pair("non-existent-pair");
        assert!(matches!(result, Err(PairError::PairNotFound(_))));
        
        // Thêm liquidity cho pair không tồn tại
        let result = manager.add_liquidity("non-existent-pair", 100.0, 200.0);
        assert!(matches!(result, Err(PairError::PairNotFound(_))));
        
        // Tạo pair
        let pair_id = manager.create_pair("0xToken1", "0xToken2", "TOKEN1-TOKEN2").unwrap();
        
        // Thêm liquidity âm
        let result = manager.add_liquidity(&pair_id, -100.0, 200.0);
        assert!(matches!(result, Err(PairError::InsufficientLiquidity { .. })));
        
        // Rút quá nhiều liquidity
        let result = manager.remove_liquidity(&pair_id, 100.0, 200.0);
        assert!(matches!(result, Err(PairError::InsufficientLiquidity { .. })));
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairUpdateRequest {
    pub fee_percent: Option<f64>,
    pub status: Option<PairStatus>,
    pub price_oracle: Option<f64>,
    pub price_impact_limit: Option<f64>,
    pub min_liquidity: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairValidation {
    pub min_liquidity: f64,
    pub max_fee_percent: f64,
    pub max_price_impact: f64,
    pub min_token_a_amount: f64,
    pub min_token_b_amount: f64,
    pub price_deviation_threshold: f64,
}

impl Default for PairValidation {
    fn default() -> Self {
        PairValidation {
            min_liquidity: 1.0,
            max_fee_percent: 0.05, // 5%
            max_price_impact: 0.05, // 5%
            min_token_a_amount: 0.001,
            min_token_b_amount: 0.001,
            price_deviation_threshold: 0.1, // 10%
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum PairStatus {
    Active,
    Inactive,
    Paused,
}

/// Thông tin về trading pair
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingPair {
    pub id: String,
    pub token_a: String,
    pub token_b: String,
    pub reserve_a: f64,
    pub reserve_b: f64,
    pub fee_percent: f64,
    pub status: PairStatus,
    pub price_oracle: f64,  // Giá tham chiếu từ oracle
    pub price_impact_limit: f64, // Giới hạn tác động giá (%)
    pub min_liquidity: f64, // Thanh khoản tối thiểu
    pub created_at: u64,
    pub updated_at: u64,
}

impl TradingPair {
    pub fn calculate_price(&self) -> f64 {
        if self.reserve_b == 0.0 {
            return self.price_oracle; // Sử dụng giá oracle nếu không có thanh khoản
        }
        self.reserve_a / self.reserve_b
    }
}
