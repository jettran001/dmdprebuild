//! # Token Service
//! 
//! Module này cung cấp dịch vụ quản lý token đơn giản.
//!
//! ## Mối quan hệ với AuthService
//!
//! Module này được thiết kế để sử dụng độc lập hoặc kết hợp với AuthService trong auth_middleware.rs:
//! - TokenService tập trung vào việc quản lý token đơn giản không phải JWT, phù hợp với các tình huống yêu cầu bảo mật thấp hơn
//! - AuthService trong auth_middleware.rs cung cấp giải pháp xác thực toàn diện, bao gồm JWT, API Key và Simple Token
//! - TokenService có thể được sử dụng cho các dịch vụ nội bộ hoặc các tình huống không yêu cầu JWT
//! - Lưu ý: Khi phát triển API endpoint mới, nên sử dụng một trong hai service để tránh xung đột

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use uuid::Uuid;
use serde::{Serialize, Deserialize};
use tokio::sync::RwLock;
use std::collections::HashMap;
use crate::security::auth_middleware::{AuthError, AuthInfo, UserRole, AuthType};

/// Cấu hình cho Token Service
#[derive(Debug, Clone)]
pub struct TokenConfig {
    pub expiry_seconds: u64,
    pub max_tokens_per_user: usize,
    pub cleanup_interval_seconds: u64,
}

impl Default for TokenConfig {
    fn default() -> Self {
        Self {
            expiry_seconds: 3600,
            max_tokens_per_user: 5,
            cleanup_interval_seconds: 900,
        }
    }
}

/// Thông tin token
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TokenInfo {
    user_id: String,
    role: UserRole,
    created_at: u64,
    expires_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<HashMap<String, String>>,
}

/// Token Service
pub struct TokenService {
    config: Arc<TokenConfig>,
    tokens: Arc<RwLock<HashMap<String, TokenInfo>>>,
    user_tokens: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl TokenService {
    /// Tạo TokenService mới
    pub fn new(config: TokenConfig) -> Self {
        let service = Self {
            config: Arc::new(config),
            tokens: Arc::new(RwLock::new(HashMap::new())),
            user_tokens: Arc::new(RwLock::new(HashMap::new())),
        };
        
        // Khởi động task dọn dẹp token hết hạn
        service.start_cleanup();
        
        service
    }
    
    /// Khởi động task dọn dẹp token hết hạn
    fn start_cleanup(&self) {
        let tokens = self.tokens.clone();
        let user_tokens = self.user_tokens.clone();
        let interval = self.config.cleanup_interval_seconds;
        
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(interval));
            
            loop {
                interval.tick().await;
                
                if let Err(e) = Self::cleanup_expired_tokens(tokens.clone(), user_tokens.clone()).await {
                    tracing::error!("Error cleaning up expired tokens: {}", e);
                }
            }
        });
    }
    
    /// Dọn dẹp token hết hạn
    async fn cleanup_expired_tokens(
        tokens: Arc<RwLock<HashMap<String, TokenInfo>>>,
        user_tokens: Arc<RwLock<HashMap<String, Vec<String>>>>,
    ) -> Result<usize, AuthError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| AuthError::GenericError(format!("System time error: {}", e)))?
            .as_secs();
        
        let mut tokens_to_remove = Vec::new();
        
        // Tìm các token hết hạn
        {
            let tokens_guard = tokens.read().await;
            for (token, info) in tokens_guard.iter() {
                if info.expires_at < now {
                    tokens_to_remove.push((token.clone(), info.user_id.clone()));
                }
            }
        }
        
        if tokens_to_remove.is_empty() {
            return Ok(0);
        }
        
        // Xóa các token hết hạn
        {
            let mut tokens_guard = tokens.write().await;
            let mut user_tokens_guard = user_tokens.write().await;
            
            for (token, user_id) in &tokens_to_remove {
                tokens_guard.remove(token);
                
                if let Some(user_tokens) = user_tokens_guard.get_mut(user_id) {
                    user_tokens.retain(|t| t != token);
                    
                    // Xóa user khỏi map nếu không còn token nào
                    if user_tokens.is_empty() {
                        user_tokens_guard.remove(user_id);
                    }
                }
            }
        }
        
        Ok(tokens_to_remove.len())
    }
    
    /// Tạo token mới
    pub async fn create_token(
        &self,
        user_id: &str,
        role: UserRole,
        metadata: Option<HashMap<String, String>>,
    ) -> Result<String, AuthError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| AuthError::GenericError(format!("System time error: {}", e)))?
            .as_secs();
        
        let expires_at = now + self.config.expiry_seconds;
        let token = Uuid::new_v4().to_string();
        
        let token_info = TokenInfo {
            user_id: user_id.to_string(),
            role,
            created_at: now,
            expires_at,
            metadata,
        };
        
        // Lưu token
        {
            let mut tokens_guard = self.tokens.write().await;
            tokens_guard.insert(token.clone(), token_info);
        }
        
        // Cập nhật user_tokens
        {
            let mut user_tokens_guard = self.user_tokens.write().await;
            let user_tokens = user_tokens_guard.entry(user_id.to_string()).or_insert_with(Vec::new);
            
            // Kiểm tra số lượng token của user
            if user_tokens.len() >= self.config.max_tokens_per_user {
                // Xóa token cũ nhất
                if let Some(oldest_token) = user_tokens.first().cloned() {
                    user_tokens.remove(0);
                    
                    let mut tokens_guard = self.tokens.write().await;
                    tokens_guard.remove(&oldest_token);
                }
            }
            
            user_tokens.push(token.clone());
        }
        
        Ok(token)
    }
    
    /// Xác thực token
    pub async fn validate_token(&self, token: &str) -> Result<AuthInfo, AuthError> {
        let tokens_guard = self.tokens.read().await;
        
        let token_info = tokens_guard.get(token)
            .ok_or_else(|| AuthError::InvalidToken("Token không hợp lệ".to_string()))?;
        
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| AuthError::GenericError(format!("System time error: {}", e)))?
            .as_secs();
        
        if token_info.expires_at < now {
            return Err(AuthError::ExpiredToken);
        }
        
        Ok(AuthInfo {
            user_id: token_info.user_id.clone(),
            name: None,
            role: token_info.role.clone(),
            expires_at: Some(token_info.expires_at),
            ip_address: None,
            token_type: AuthType::Simple,
            metadata: token_info.metadata.clone(),
            additional: HashMap::new(),
        })
    }
    
    /// Thu hồi token
    pub async fn revoke_token(&self, token: &str) -> Result<(), AuthError> {
        let mut tokens_guard = self.tokens.write().await;
        
        let token_info = tokens_guard.get(token)
            .ok_or_else(|| AuthError::InvalidToken("Token không tồn tại".to_string()))?;
        
        let user_id = token_info.user_id.clone();
        
        tokens_guard.remove(token);
        
        // Cập nhật user_tokens
        let mut user_tokens_guard = self.user_tokens.write().await;
        if let Some(user_tokens) = user_tokens_guard.get_mut(&user_id) {
            user_tokens.retain(|t| t != token);
            
            // Xóa user khỏi map nếu không còn token nào
            if user_tokens.is_empty() {
                user_tokens_guard.remove(&user_id);
            }
        }
        
        Ok(())
    }
    
    /// Lấy tất cả token của user
    pub async fn get_user_tokens(&self, user_id: &str) -> Vec<String> {
        let user_tokens_guard = self.user_tokens.read().await;
        
        user_tokens_guard.get(user_id)
            .cloned()
            .unwrap_or_default()
    }
    
    /// Thu hồi tất cả token của user
    pub async fn revoke_user_tokens(&self, user_id: &str) -> Result<usize, AuthError> {
        let mut user_tokens_guard = self.user_tokens.write().await;
        
        let tokens = user_tokens_guard.remove(user_id)
            .unwrap_or_default();
        
        if tokens.is_empty() {
            return Ok(0);
        }
        
        // Xóa token từ map tokens
        let mut tokens_guard = self.tokens.write().await;
        for token in &tokens {
            tokens_guard.remove(token);
        }
        
        Ok(tokens.len())
    }
} 