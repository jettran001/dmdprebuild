use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;
use anyhow::Result;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{Middleware, Request, Response, MiddlewareError};

/// Authentication middleware
pub struct AuthMiddleware {
    /// Cấu hình
    config: AuthConfig,
    
    /// Token store
    token_store: Arc<RwLock<HashMap<String, TokenInfo>>>,
}

/// Cấu hình authentication
pub struct AuthConfig {
    /// JWT secret key
    pub jwt_secret: String,
    
    /// JWT expiration time (seconds)
    pub jwt_expiration_seconds: u64,
    
    /// Refresh token expiration time (seconds)
    pub refresh_expiration_seconds: u64,
    
    /// Các routes được miễn xác thực
    pub exempt_routes: Vec<String>,
    
    /// Sử dụng header Authorization
    pub use_auth_header: bool,
    
    /// Sử dụng cookie
    pub use_cookie: bool,
}

/// Thông tin token
struct TokenInfo {
    /// Token
    _token: String,
    
    /// Refresh token
    refresh_token: String,
    
    /// Thời điểm hết hạn
    expires_at: SystemTime,
    
    /// Thời điểm tạo
    _created_at: SystemTime,
    
    /// User ID
    user_id: String,
}

/// JWT claims
#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    /// Subject (user ID)
    sub: String,
    
    /// Name
    name: Option<String>,
    
    /// Roles
    roles: Vec<String>,
    
    /// Issued at
    iat: i64,
    
    /// Expiration time
    exp: i64,
}

/// Kết quả xác thực
pub enum AuthResult {
    /// Thành công, kèm theo thông tin user
    Success(UserInfo),
    
    /// Thất bại
    Failure(String),
}

/// Thông tin user
#[derive(Debug, Clone)]
pub struct UserInfo {
    /// User ID
    pub id: String,
    
    /// Username
    pub username: String,
    
    /// Roles
    pub roles: Vec<String>,
}

#[async_trait::async_trait]
impl Middleware for AuthMiddleware {
    async fn process_request(&self, request: &mut Request) -> Result<(), MiddlewareError> {
        // Kiểm tra xem route có được miễn xác thực không
        for exempt in &self.config.exempt_routes {
            if request.uri.starts_with(exempt) {
                return Ok(());
            }
        }
        
        // Lấy token từ header hoặc cookie
        let token = if self.config.use_auth_header {
            request.headers.get("Authorization")
                .and_then(|h| h.strip_prefix("Bearer "))
                .map(|s| s.to_string())
        } else if self.config.use_cookie {
            request.headers.get("Cookie")
                .and_then(|c| {
                    c.split(';')
                        .find_map(|part| {
                            let part = part.trim();
                            if part.starts_with("token=") {
                                part.strip_prefix("token=").map(|s| s.to_string())
                            } else {
                                None
                            }
                        })
                })
        } else {
            None
        };
        
        match token {
            Some(token) => {
                // Xác thực token
                match self.validate_token(&token).await {
                    AuthResult::Success(user_info) => {
                        // Thêm thông tin user vào request
                        request.headers.insert("X-User-ID".to_string(), user_info.id.clone());
                        request.headers.insert("X-Username".to_string(), user_info.username.clone());
                        request.headers.insert("X-User-Roles".to_string(), user_info.roles.join(","));
                        Ok(())
                    },
                    AuthResult::Failure(reason) => {
                        Err(MiddlewareError::AuthenticationFailed(reason))
                    }
                }
            },
            None => {
                Err(MiddlewareError::AuthenticationFailed("No authentication token provided".to_string()))
            }
        }
    }
    
    async fn process_response(&self, _response: &mut Response) -> Result<(), MiddlewareError> {
        // Không cần xử lý gì với response
        Ok(())
    }
}

impl AuthMiddleware {
    /// Tạo mới authentication middleware
    pub fn new(config: AuthConfig) -> Self {
        Self {
            config,
            token_store: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Tạo token mới
    pub async fn create_token(&self, user: &UserInfo) -> Result<(String, String)> {
        let now = SystemTime::now();
        let exp = now + Duration::from_secs(self.config.jwt_expiration_seconds);
        
        let claims = Claims {
            sub: user.id.clone(),
            name: Some(user.username.clone()),
            roles: user.roles.clone(),
            iat: now.duration_since(UNIX_EPOCH)?.as_secs() as i64,
            exp: exp.duration_since(UNIX_EPOCH)?.as_secs() as i64,
        };
        
        // Tạo JWT
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.config.jwt_secret.as_bytes())
        )?;
        
        // Tạo refresh token
        let refresh_token = uuid::Uuid::new_v4().to_string();
        
        // Lưu vào token store
        let token_info = TokenInfo {
            _token: token.clone(),
            refresh_token: refresh_token.clone(),
            expires_at: exp,
            _created_at: now,
            user_id: user.id.clone(),
        };
        
        let mut store = self.token_store.write().await;
        store.insert(token.clone(), token_info);
        
        Ok((token, refresh_token))
    }
    
    /// Xác thực token
    pub async fn validate_token(&self, token: &str) -> AuthResult {
        // Kiểm tra trong token store
        {
            let store = self.token_store.read().await;
            if let Some(info) = store.get(token) {
                if info.expires_at > SystemTime::now() {
                    // Token còn hiệu lực
                    // Giả lập lấy thông tin user từ database
                    let user_info = UserInfo {
                        id: info.user_id.clone(),
                        username: "user".to_string(), // Giả lập
                        roles: vec!["user".to_string()], // Giả lập
                    };
                    
                    return AuthResult::Success(user_info);
                }
            }
        }
        
        // Xác thực JWT
        let validation = Validation::new(Algorithm::HS256);
        
        match decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.config.jwt_secret.as_bytes()),
            &validation
        ) {
            Ok(token_data) => {
                let claims = token_data.claims;
                
                // Giả lập lấy thông tin user từ database
                let user_info = UserInfo {
                    id: claims.sub,
                    username: claims.name.unwrap_or_else(|| "unknown".to_string()),
                    roles: claims.roles,
                };
                
                AuthResult::Success(user_info)
            },
            Err(err) => {
                AuthResult::Failure(format!("Invalid token: {}", err))
            }
        }
    }
    
    /// Làm mới token
    pub async fn refresh_token(&self, refresh_token: &str) -> Result<(String, String)> {
        // Tìm token dựa trên refresh token
        let user_id = {
            let store = self.token_store.read().await;
            let mut user_id = None;
            
            for info in store.values() {
                if info.refresh_token == refresh_token {
                    user_id = Some(info.user_id.clone());
                    break;
                }
            }
            
            user_id
        };
        
        if let Some(user_id) = user_id {
            // Giả lập lấy thông tin user từ database
            let user_info = UserInfo {
                id: user_id,
                username: "user".to_string(), // Giả lập
                roles: vec!["user".to_string()], // Giả lập
            };
            
            // Tạo token mới
            self.create_token(&user_info).await
        } else {
            Err(anyhow::anyhow!("Invalid refresh token"))
        }
    }
    
    /// Hủy token
    pub async fn revoke_token(&self, token: &str) -> bool {
        let mut store = self.token_store.write().await;
        store.remove(token).is_some()
    }
}

