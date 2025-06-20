use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use anyhow::{Result, Context, anyhow};
use log::{debug, error, info, warn};

use crate::defi::token::standard::{TokenStandard, TokenType, TokenInfo};
use crate::defi::blockchain::{BlockchainProvider, ChainId};
use crate::defi::contracts::{ContractMetadata, ContractType};
use crate::defi::error::TokenError;
use crate::utils::validation::{validate_address, ValidationError};

/// Quản lý token cho ví
pub struct TokenManager {
    /// Token cache theo địa chỉ
    tokens: RwLock<HashMap<String, TokenInfo>>,
    /// Provider cho các blockchain khác nhau
    providers: RwLock<HashMap<ChainId, Arc<dyn BlockchainProvider>>>,
    /// Lock cho các thao tác quan trọng
    operation_lock: Mutex<()>,
    /// Số lần thử lại tối đa
    max_retries: u32,
}

impl TokenManager {
    /// Khởi tạo token manager
    pub fn new() -> Self {
        Self {
            tokens: RwLock::new(HashMap::new()),
            providers: RwLock::new(HashMap::new()),
            operation_lock: Mutex::new(()),
            max_retries: 3,
        }
    }
    
    /// Đặt provider cho một blockchain cụ thể
    pub async fn set_provider(&self, chain_id: ChainId, provider: Arc<dyn BlockchainProvider>) {
        let mut providers = self.providers.write().await;
        providers.insert(chain_id, provider);
        info!("Đã đặt provider cho chain: {}", chain_id);
    }
    
    /// Kiểm tra địa chỉ token có hợp lệ không
    pub async fn validate_token_address(&self, address: &str, chain_id: &ChainId) -> Result<(), TokenError> {
        // Kiểm tra địa chỉ không trống
        if address.trim().is_empty() {
            error!("Địa chỉ token rỗng");
            return Err(TokenError::InvalidAddress("Địa chỉ token rỗng".to_string()));
        }
        
        // Kiểm tra địa chỉ có đúng định dạng không
        if let Err(e) = validate_address(address) {
            error!("Địa chỉ token không hợp lệ: {}, lỗi: {}", address, e);
            return Err(TokenError::InvalidAddress(format!("Địa chỉ không hợp lệ: {}", e)));
        }
        
        // Kiểm tra blockchain provider tồn tại
        let providers = self.providers.read().await;
        let provider = providers.get(chain_id).ok_or_else(|| {
            error!("Không tìm thấy provider cho chain: {}", chain_id);
            TokenError::ProviderNotFound(chain_id.clone())
        })?;
        
        // Kiểm tra địa chỉ có tồn tại trên blockchain
        if !provider.is_contract_address(address).await.map_err(|e| {
            error!("Lỗi khi kiểm tra địa chỉ token {}: {}", address, e);
            TokenError::ProviderError(format!("Lỗi provider: {}", e))
        })? {
            warn!("Địa chỉ {} không phải là contract trên chain {}", address, chain_id);
            return Err(TokenError::NotAContract(address.to_string()));
        }
        
        Ok(())
    }
    
    /// Thêm token vào manager
    pub async fn add_token(&self, address: &str, chain_id: ChainId, token_type: TokenType) -> Result<TokenInfo, TokenError> {
        // Tạo key cho token
        let key = format!("{}:{}", chain_id, address);
        
        // Kiểm tra token đã tồn tại chưa trong cache
        {
            let tokens = self.tokens.read().await;
            if let Some(token) = tokens.get(&key) {
                debug!("Token {} đã tồn tại trong cache, trả về thông tin", address);
                return Ok(token.clone());
            }
        }
        
        // Kiểm tra địa chỉ token
        self.validate_token_address(address, &chain_id).await?;
        
        // Lấy lock để đảm bảo chỉ một thao tác được thực hiện
        let _guard = self.operation_lock.lock().await;
        
        // Kiểm tra lại sau khi có lock (tránh race condition)
        {
            let tokens = self.tokens.read().await;
            if let Some(token) = tokens.get(&key) {
                info!("Token {} đã tồn tại (kiểm tra lại), trả về thông tin", address);
                return Ok(token.clone());
            }
        }
        
        // Tạo token standard dựa trên loại
        let standard = TokenStandard::from_type(token_type);
        
        // Lấy provider
        let providers = self.providers.read().await;
        let provider = providers.get(&chain_id).ok_or_else(|| {
            error!("Không tìm thấy provider cho chain: {}", chain_id);
            TokenError::ProviderNotFound(chain_id)
        })?;
        
        // Lấy thông tin token từ blockchain với retry
        let mut retry_count = 0;
        let mut last_error = None;
        
        while retry_count < self.max_retries {
            debug!("Lấy thông tin token {} (lần thử: {})", address, retry_count + 1);
            
            match standard.get_token_info(address, provider.clone()).await {
                Ok(mut token_info) => {
                    // Thêm chain_id vào thông tin token
                    token_info.chain_id = chain_id.clone();
                    
                    // Lưu token vào cache
                    let mut tokens = self.tokens.write().await;
                    tokens.insert(key, token_info.clone());
                    
                    info!("Đã thêm token {} ({}) vào chain {}", 
                         token_info.name, address, chain_id);
                    
                    return Ok(token_info);
                },
                Err(e) => {
                    warn!("Lỗi khi lấy thông tin token {} (lần thử {}): {}", 
                         address, retry_count + 1, e);
                    last_error = Some(e);
                    retry_count += 1;
                    
                    // Đợi trước khi thử lại
                    if retry_count < self.max_retries {
                        tokio::time::sleep(tokio::time::Duration::from_millis(500 * (retry_count as u64))).await;
                    }
                }
            }
        }
        
        error!("Không thể lấy thông tin token {} sau {} lần thử", 
              address, self.max_retries);
        
        Err(TokenError::FetchFailed(
            address.to_string(), 
            match last_error {
                Some(e) => e.to_string(),
                None => {
                    warn!("Fetch token {} failed but no error captured", address);
                    "Unknown error".to_string()
                }
            }
        ))
    }
    
    /// Lấy thông tin token
    pub async fn get_token(&self, address: &str, chain_id: &ChainId) -> Result<TokenInfo, TokenError> {
        // Kiểm tra địa chỉ không trống
        if address.trim().is_empty() {
            error!("Địa chỉ token rỗng");
            return Err(TokenError::InvalidAddress("Địa chỉ token rỗng".to_string()));
        }
        
        // Tạo key cho token
        let key = format!("{}:{}", chain_id, address);
        
        // Kiểm tra trong cache
        let tokens = self.tokens.read().await;
        
        if let Some(token) = tokens.get(&key) {
            debug!("Lấy thông tin token {} từ cache", address);
            return Ok(token.clone());
        }
        
        // Giải phóng lock trước khi gọi add_token
        drop(tokens);
        
        // Thử đoán loại token và thêm vào manager
        info!("Token {} chưa có trong cache, thử tự động phát hiện", address);
        
        // Thử lần lượt các loại token
        for token_type in [TokenType::ERC20, TokenType::ERC721, TokenType::ERC1155].iter() {
            match self.add_token(address, chain_id.clone(), *token_type).await {
                Ok(token_info) => {
                    info!("Đã phát hiện token {} là loại {:?}", address, token_type);
                    return Ok(token_info);
                },
                Err(TokenError::StandardMismatch(_)) => {
                    debug!("Token {} không phải loại {:?}, thử loại khác", address, token_type);
                    continue;
                },
                Err(e) => {
                    error!("Lỗi khi thêm token {}: {}", address, e);
                    return Err(e);
                }
            }
        }
        
        error!("Không thể xác định loại token cho địa chỉ {} trên chain {}", address, chain_id);
        Err(TokenError::UnknownTokenType(address.to_string()))
    }
    
    /// Lấy metadata cho token contract
    pub async fn get_token_metadata(&self, address: &str, chain_id: &ChainId) -> Result<ContractMetadata, TokenError> {
        // Kiểm tra địa chỉ không trống
        if address.trim().is_empty() {
            error!("Địa chỉ token rỗng");
            return Err(TokenError::InvalidAddress("Địa chỉ token rỗng".to_string()));
        }
        
        // Lấy thông tin token
        let token_info = self.get_token(address, chain_id).await?;
        
        // Tạo metadata
        let metadata = ContractMetadata {
            name: token_info.name.clone(),
            address: address.to_string(),
            chain_id: chain_id.clone(),
            contract_type: match token_info.token_type {
                TokenType::ERC20 => ContractType::Token,
                TokenType::ERC721 => ContractType::NFT,
                TokenType::ERC1155 => ContractType::MultiToken,
                _ => ContractType::Unknown,
            },
            verified: true, // Token đã được quản lý bởi TokenManager nên coi như verified
            audited: false, // Mặc định chưa audit
        };
        
        Ok(metadata)
    }
    
    /// Xóa token khỏi cache
    pub async fn remove_token(&self, address: &str, chain_id: &ChainId) -> Result<(), TokenError> {
        // Kiểm tra địa chỉ không trống
        if address.trim().is_empty() {
            error!("Địa chỉ token rỗng");
            return Err(TokenError::InvalidAddress("Địa chỉ token rỗng".to_string()));
        }
        
        let key = format!("{}:{}", chain_id, address);
        let mut tokens = self.tokens.write().await;
        
        if tokens.remove(&key).is_some() {
            info!("Đã xóa token {} khỏi chain {}", address, chain_id);
            Ok(())
        } else {
            warn!("Không tìm thấy token {} trên chain {}", address, chain_id);
            Err(TokenError::TokenNotFound(address.to_string()))
        }
    }
    
    /// Xóa tất cả token khỏi cache
    pub async fn clear_cache(&self) {
        let mut tokens = self.tokens.write().await;
        let count = tokens.len();
        tokens.clear();
        info!("Đã xóa {} token khỏi cache", count);
    }
    
    /// Lấy danh sách tất cả token đã được cache
    pub async fn get_all_tokens(&self) -> HashMap<String, TokenInfo> {
        let tokens = self.tokens.read().await;
        tokens.clone()
    }
    
    /// Lấy danh sách token theo chain
    pub async fn get_tokens_by_chain(&self, chain_id: &ChainId) -> HashMap<String, TokenInfo> {
        let tokens = self.tokens.read().await;
        let prefix = format!("{}:", chain_id);
        
        tokens.iter()
            .filter(|(key, _)| key.starts_with(&prefix))
            .map(|(key, token)| {
                let address = match key.split(':').nth(1) {
                    Some(addr) => addr,
                    None => {
                        warn!("Token key {} does not contain address part", key);
                        ""
                    }
                };
                (address.to_string(), token.clone())
            })
            .collect()
    }
} 