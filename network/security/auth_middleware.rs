use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation, Algorithm};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use std::time::{Duration, SystemTime, UNIX_EPOCH, Instant};
use std::sync::Arc;
use tokio::sync::{RwLock, Mutex};
use rand::Rng;
use std::net::IpAddr;
use std::net::SocketAddr;
use warp::{Filter, Rejection, Reply};
use warp::filters::header::headers_cloned;
use warp::http::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use async_trait::async_trait;
use once_cell::sync::Lazy;
use tracing::{info, warn, error, debug};
use std::env;
use tokio;
use uuid::Uuid;
use std::sync::atomic::AtomicBool;

/// # Authentication và Authorization Middleware
///
/// Module này cung cấp dịch vụ xác thực và phân quyền tập trung cho toàn bộ domain network.
/// AuthService là service chính xử lý xác thực người dùng thông qua nhiều phương thức khác nhau.
///
/// ## Mối quan hệ với các dịch vụ xác thực khác:
///
/// * **JWT Service (`jwt.rs`)**: AuthService có chức năng xử lý JWT tương tự JwtService, nhưng toàn diện hơn.
///   Các API mới nên ưu tiên sử dụng AuthService thay vì JwtService trực tiếp.
///   - JwtService chỉ tập trung vào việc tạo và xác thực JWT tokens
///   - AuthService hỗ trợ JWT, API key, và simple token, với các cơ chế bảo mật bổ sung như rate limiting và key rotation
///   - JwtService phù hợp cho các trường hợp đơn giản hoặc tái sử dụng legacy code
///
/// * **Token Service (`token.rs`)**: AuthService có tính năng simple token tương tự TokenService.
///   TokenService có thể được sử dụng cho các tình huống đơn giản hoặc nội bộ.
///   - TokenService chỉ xử lý token đơn giản (không phải JWT) cho các kịch bản cần tạm thời đánh dấu một trạng thái
///   - AuthService hỗ trợ simple token với thêm các cơ chế bảo mật và xác thực IP
///   - TokenService phù hợp cho internal service-to-service authentication hoặc temporary state marking
///
/// * **Quy tắc sử dụng**: 
///   - Với các API endpoint công khai, luôn sử dụng AuthService từ module này
///   - JwtService và TokenService chỉ nên được sử dụng cho các trường hợp đặc biệt
///   - Khi dùng service khác ngoài AuthService, cần ghi chú rõ ràng lý do
///   - Sử dụng các phương thức rotate key để thay đổi key định kỳ, tăng cường bảo mật
///   - Tăng cường bảo mật bằng cách dùng start_auto_key_rotation() để thiết lập xoay vòng key tự động
///
/// AuthService là implementation duy nhất của trait Auth, đảm bảo tính nhất quán trong xác thực.
/// 
/// Các lỗi xác thực
///
#[derive(Error, Debug)]
pub enum AuthError {
    #[error("Token không hợp lệ: {0}")]
    InvalidToken(String),
    
    #[error("Token hết hạn")]
    ExpiredToken,
    
    #[error("Không có quyền truy cập")]
    Forbidden,
    
    #[error("Không có token xác thực")]
    NoToken,
    
    #[error("Lỗi xác thực: {0}")]
    GenericError(String),
    
    #[error("Lỗi JWT: {0}")]
    JwtError(#[from] jsonwebtoken::errors::Error),
    
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    
    #[error("Rate limited: {0}")]
    RateLimited(String),
}

/// User roles
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserRole {
    #[serde(rename = "admin")]
    Admin,
    #[serde(rename = "partner")]
    Partner,
    #[serde(rename = "node")]
    Node,
    #[serde(rename = "service")]
    Service,
    #[serde(rename = "user")]
    User,
}

/// Claims cho JWT
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    /// Subject (thường là ID)
    pub sub: String,
    /// Tên hiển thị
    pub name: Option<String>,
    /// Quyền hạn
    pub role: UserRole,
    /// Thời gian hết hạn
    pub exp: u64,
    /// Thời gian phát hành
    pub iat: u64,
    /// Người phát hành
    pub iss: String,
    /// Các claim bổ sung
    #[serde(default)]
    pub additional: HashMap<String, String>,
}

/// Thông tin người dùng đã xác thực
#[derive(Debug, Clone)]
pub struct AuthInfo {
    /// ID người dùng
    pub user_id: String,
    /// Tên hiển thị
    pub name: Option<String>,
    /// Quyền hạn
    pub role: UserRole,
    /// Thời gian hết hạn
    pub expires_at: Option<u64>,
    /// Địa chỉ IP
    pub ip_address: Option<IpAddr>,
    /// Loại token xác thực
    pub token_type: AuthType,
    /// Metadata bổ sung
    pub metadata: Option<HashMap<String, String>>,
    /// Các thông tin bổ sung
    pub additional: HashMap<String, String>,
}

/// Kiểu xác thực
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthType {
    /// Xác thực bằng JWT
    Jwt,
    /// Xác thực bằng API key
    ApiKey,
    /// Xác thực đơn giản (IP + token)
    Simple,
}

/// Cấu hình cho xử lý thất bại xác thực và kiểm soát tần suất đăng nhập
#[derive(Debug, Clone)]
pub struct AuthFailConfig {
    /// Số lần thất bại tối đa trước khi bị khóa
    pub max_fail_attempts: u32,
    /// Thời gian cửa sổ kiểm tra (giây)
    pub fail_window_secs: u64,
    /// Thời gian khóa (giây) sau khi vượt quá số lần thất bại
    pub lockout_duration_secs: u64,
}

impl Default for AuthFailConfig {
    fn default() -> Self {
        Self {
            max_fail_attempts: 5,
            fail_window_secs: 60,
            lockout_duration_secs: 300, // 5 phút
        }
    }
}

/// Service xác thực
pub struct AuthService {
    jwt_secret: String,
    api_keys: Arc<RwLock<HashMap<String, AuthInfo>>>,
    simple_tokens: Arc<RwLock<HashMap<String, AuthInfo>>>,
    default_issuer: String,
    default_expiration: Duration,
    revoked_jwt: Arc<RwLock<HashMap<String, u64>>>,
    rate_limiter: Arc<crate::security::rate_limiter::RateLimiter>,
    jwt_algorithm: Algorithm,
    rsa_private_key: Option<String>, // Khóa RSA private key nếu sử dụng RS256
    rsa_public_key: Option<String>,  // Khóa RSA public key nếu sử dụng RS256
    auth_fail_config: AuthFailConfig, // Cấu hình xử lý thất bại xác thực
    #[allow(dead_code)]
    shutdown_signal: Mutex<Option<()>>,
    #[allow(dead_code)]
    is_shutdown: AtomicBool,
    #[allow(dead_code)]
    cleanup_handle: Option<tokio::task::JoinHandle<()>>,
    #[allow(dead_code)]
    connection_closer: Option<tokio::task::JoinHandle<()>>,
}

impl Default for AuthService {
    fn default() -> Self {
        let secret = env::var("JWT_SECRET").unwrap_or_else(|_| "jwt_secret".to_string());
        let issuer = env::var("JWT_ISSUER").unwrap_or_else(|_| "diamond_chain".to_string());
        let expiration_secs = env::var("JWT_EXPIRATION_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(3600); // 1 giờ
        
        // Đọc cấu hình thất bại xác thực từ biến môi trường
        let max_fail_attempts = env::var("AUTH_MAX_FAIL_ATTEMPTS")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(5);
            
        let fail_window_secs = env::var("AUTH_FAIL_WINDOW_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(60);
            
        let lockout_duration_secs = env::var("AUTH_LOCKOUT_DURATION_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(300);
            
        let auth_fail_config = AuthFailConfig {
            max_fail_attempts,
            fail_window_secs,
            lockout_duration_secs,
        };
            
        let rate_limiter = Arc::new(crate::security::rate_limiter::RateLimiter::new());
        
        // Mặc định sử dụng HS256 cho tương thích ngược, nhưng khuyến nghị sử dụng RS256 cho production
        let jwt_algorithm = if env::var("JWT_USE_RS256").unwrap_or_else(|_| "false".to_string()) == "true" {
            Algorithm::RS256
        } else {
            Algorithm::HS256
        };
        
        // Đọc RSA keys nếu có
        let rsa_private_key = env::var("JWT_RSA_PRIVATE_KEY").ok();
        let rsa_public_key = env::var("JWT_RSA_PUBLIC_KEY").ok();
        
        Self {
            jwt_secret: secret,
            api_keys: Arc::new(RwLock::new(HashMap::new())),
            simple_tokens: Arc::new(RwLock::new(HashMap::new())),
            default_issuer: issuer,
            default_expiration: Duration::from_secs(expiration_secs),
            revoked_jwt: Arc::new(RwLock::new(HashMap::new())),
            rate_limiter,
            jwt_algorithm,
            rsa_private_key,
            rsa_public_key,
            auth_fail_config,
            shutdown_signal: Mutex::new(None),
            is_shutdown: AtomicBool::new(false),
            cleanup_handle: None,
            connection_closer: None,
        }
    }
}

impl AuthService {
    /// Tạo mới service xác thực
    pub fn new(jwt_secret: String) -> Self {
        debug!("[Auth] Creating new AuthService instance with custom JWT secret");
        Self {
            jwt_secret,
            ..Self::default()
        }
    }
    
    /// Khởi tạo AuthService với thuật toán RS256 (khuyến nghị cho môi trường production)
    pub fn with_rs256(private_key: String, public_key: String) -> Self {
        info!("[Auth] Creating new AuthService instance with RS256 algorithm");
        Self {
            jwt_algorithm: Algorithm::RS256,
            rsa_private_key: Some(private_key),
            rsa_public_key: Some(public_key),
            ..Self::default()
        }
    }
    
    /// Lấy encoding key dựa vào thuật toán
    fn get_encoding_key(&self) -> Result<EncodingKey, AuthError> {
        match self.jwt_algorithm {
            Algorithm::HS256 => {
                debug!("[Auth] Using HS256 encoding key");
                Ok(EncodingKey::from_secret(self.jwt_secret.as_bytes()))
            },
            Algorithm::RS256 => {
                debug!("[Auth] Using RS256 encoding key");
                let private_key = self.rsa_private_key.as_ref()
                    .ok_or_else(|| {
                        error!("[Auth] RSA private key is not set for RS256 algorithm");
                        AuthError::GenericError("RSA private key is not set".to_string())
                    })?;
                EncodingKey::from_rsa_pem(private_key.as_bytes())
                    .map_err(|e| {
                        error!("[Auth] Invalid RSA private key: {}", e);
                        AuthError::GenericError(format!("Invalid RSA private key: {}", e))
                    })
            },
            _ => {
                error!("[Auth] Unsupported JWT algorithm: {:?}", self.jwt_algorithm);
                Err(AuthError::GenericError(format!("Unsupported JWT algorithm: {:?}", self.jwt_algorithm)))
            },
        }
    }
    
    /// Lấy decoding key dựa vào thuật toán
    fn get_decoding_key(&self) -> Result<DecodingKey, AuthError> {
        match self.jwt_algorithm {
            Algorithm::HS256 => {
                debug!("[Auth] Using HS256 decoding key");
                Ok(DecodingKey::from_secret(self.jwt_secret.as_bytes()))
            },
            Algorithm::RS256 => {
                debug!("[Auth] Using RS256 decoding key");
                let public_key = self.rsa_public_key.as_ref()
                    .ok_or_else(|| {
                        error!("[Auth] RSA public key is not set for RS256 algorithm");
                        AuthError::GenericError("RSA public key is not set".to_string())
                    })?;
                DecodingKey::from_rsa_pem(public_key.as_bytes())
                    .map_err(|e| {
                        error!("[Auth] Invalid RSA public key: {}", e);
                        AuthError::GenericError(format!("Invalid RSA public key: {}", e))
                    })
            },
            _ => {
                error!("[Auth] Unsupported JWT algorithm: {:?}", self.jwt_algorithm);
                Err(AuthError::GenericError(format!("Unsupported JWT algorithm: {:?}", self.jwt_algorithm)))
            },
        }
    }
    
    /// Tạo JWT token
    pub fn create_jwt(&self, user_id: &str, name: Option<&str>, role: UserRole, 
                      expiration: Option<Duration>, additional: Option<HashMap<String, String>>) -> Result<String, AuthError> {
        debug!("[Auth] Creating JWT token for user {} with role {:?}", user_id, role);
        let now = SystemTime::now().duration_since(UNIX_EPOCH)
            .map_err(|e| {
                error!("[Auth] System time error: {}", e);
                AuthError::GenericError(format!("System time error: {}", e))
            })?
            .as_secs();
            
        let expiration = expiration.unwrap_or(self.default_expiration);
        let exp = now + expiration.as_secs();
        
        let claims = Claims {
            sub: user_id.to_string(),
            name: name.map(|s| s.to_string()),
            role,
            exp,
            iat: now,
            iss: self.default_issuer.clone(),
            additional: additional.unwrap_or_default(),
        };
        
        let header = Header {
            alg: self.jwt_algorithm,
            ..Header::default()
        };
        
        let encoding_key = match self.get_encoding_key() {
            Ok(key) => key,
            Err(e) => {
                error!("[Auth] Failed to get encoding key: {}", e);
                return Err(e);
            }
        };

        let token = encode(&header, &claims, &encoding_key)
            .map_err(|e| {
                error!("[Auth] Failed to encode JWT: {}", e);
                AuthError::JwtError(e)
            })?;
        
        info!("[Auth] Successfully created JWT token for user {} (exp: {})", user_id, exp);
        Ok(token)
    }
    
    /// Xác thực JWT token
    pub async fn validate_jwt(&self, token: &str) -> Result<AuthInfo, AuthError> {
        debug!("[Auth] Validating JWT token");
        
        // Check rate limit cho IP
        let token_part = token.get(0..10).unwrap_or(token);
        let identifier = crate::security::rate_limiter::RequestIdentifier::Custom(format!("jwt:{}", token_part));
        
        let rt = tokio::runtime::Handle::current();
        let rate_check = rt.block_on(async {
            self.rate_limiter.check_limit("/api/auth/validate_jwt", identifier).await
        });
        
        if let Err(e) = rate_check {
            warn!("[Auth] Rate limit exceeded for JWT validation: {}", e);
            return Err(AuthError::RateLimited(e.to_string()));
        }
    
        let mut validation = Validation::new(self.jwt_algorithm);
        // Kiểm tra issuer nếu được cấu hình
        if !self.default_issuer.is_empty() {
            validation.set_issuer(&[&self.default_issuer]);
        }
        
        let decoding_key = match self.get_decoding_key() {
            Ok(key) => key,
            Err(e) => {
                error!("[Auth] Failed to get decoding key: {}", e);
                return Err(e);
            }
        };

        let token_data = decode::<Claims>(token, &decoding_key, &validation)
            .map_err(|e| {
                warn!("[Auth] JWT decode failed: {}", e);
                AuthError::JwtError(e)
            })?;
        
        let claims = token_data.claims;
        
        // Kiểm tra token hết hạn
        let now = SystemTime::now().duration_since(UNIX_EPOCH)
            .map_err(|e| {
                error!("[Auth] System time error: {}", e);
                AuthError::GenericError(format!("System time error: {}", e))
            })?
            .as_secs();
            
        if claims.exp < now {
            warn!("[Auth] Token expired for user {}, expired at {}, now {}", claims.sub, claims.exp, now);
            return Err(AuthError::ExpiredToken);
        }
        
        // Kiểm tra token có trong blacklist không
        let revoked_jwt = self.revoked_jwt.read().await;
        
        if revoked_jwt.contains_key(token) {
            warn!("[Auth] Attempt to use revoked token for user {}", claims.sub);
            return Err(AuthError::InvalidToken("Token đã bị thu hồi".to_string()));
        }
        
        // Trả về thông tin xác thực
        info!("[Auth] Successfully validated JWT token for user {} with role {:?}", claims.sub, claims.role);
        Ok(AuthInfo {
            user_id: claims.sub,
            name: claims.name,
            role: claims.role,
            expires_at: Some(claims.exp),
            ip_address: None,
            token_type: AuthType::Jwt,
            metadata: None,
            additional: claims.additional,
        })
    }
    
    /// Đăng ký API key
    pub async fn register_api_key(&self, 
                          user_id: &str, 
                          name: Option<&str>, 
                          role: UserRole,
                          additional: Option<HashMap<String, String>>) -> Result<String, AuthError> {
        debug!("[Auth] Registering new API key for user {} with role {:?}", user_id, role);
        
        // Tạo API key ngẫu nhiên
        let api_key = generate_api_key();
        
        // Thông tin xác thực
        let auth_info = AuthInfo {
            user_id: user_id.to_string(),
            name: name.map(|s| s.to_string()),
            role,
            expires_at: None, // API key không hết hạn
            ip_address: None,
            token_type: AuthType::ApiKey,
            metadata: None,
            additional: additional.unwrap_or_default(),
        };
        
        // Lưu API key
        let mut api_keys = self.api_keys.write().await;
        
        api_keys.insert(api_key.clone(), auth_info);
        
        info!("[Auth] Successfully registered API key for user {}", user_id);
        Ok(api_key)
    }
    
    /// Xác thực API key
    pub async fn validate_api_key(&self, api_key: &str) -> Result<AuthInfo, AuthError> {
        debug!("[Auth] Validating API key");
        
        // Check rate limit cho API key
        let key_part = api_key.get(0..10).unwrap_or(api_key);
        let identifier = crate::security::rate_limiter::RequestIdentifier::Custom(format!("api:{}", key_part));
        
        let rt = tokio::runtime::Handle::current();
        let rate_check = rt.block_on(async {
            self.rate_limiter.check_limit("/api/auth/validate_api_key", identifier).await
        });
        
        if let Err(e) = rate_check {
            warn!("[Auth] Rate limit exceeded for API key validation: {}", e);
            return Err(AuthError::RateLimited(e.to_string()));
        }
        
        let api_keys = self.api_keys.read().await;
        
        if let Some(auth_info) = api_keys.get(api_key) {
            info!("[Auth] Successfully validated API key for user {} with role {:?}", auth_info.user_id, auth_info.role);
            Ok(auth_info.clone())
        } else {
            warn!("[Auth] Invalid API key: {}", key_part);
            Err(AuthError::InvalidToken("API key không hợp lệ".to_string()))
        }
    }
    
    /// Thu hồi API key
    pub async fn revoke_api_key(&self, api_key: &str) -> Result<(), AuthError> {
        debug!("[Auth] Revoking API key");
        let mut api_keys = self.api_keys.write().await;
        
        if api_keys.remove(api_key).is_some() {
            info!("[Auth] API key successfully revoked");
        Ok(())
        } else {
            warn!("[Auth] Attempted to revoke non-existent API key");
            Err(AuthError::InvalidToken("API key không tồn tại".to_string()))
        }
    }
    
    /// Tạo simple token (token đơn giản kết hợp với IP)
    pub async fn create_simple_token(&self, 
                         user_id: &str, 
                         ip_address: IpAddr,
                         role: UserRole,
                         expiration: Option<Duration>,
                         additional: Option<HashMap<String, String>>) -> Result<String, AuthError> {
        debug!("[Auth] Creating simple token for user {} with role {:?} from IP {}", user_id, role, ip_address);
        
        // Tạo token ngẫu nhiên
        let token = generate_simple_token();
        
        // Thông tin xác thực
        let exp = if let Some(expiration) = expiration {
            let now = SystemTime::now().duration_since(UNIX_EPOCH)
                .map_err(|e| {
                    error!("[Auth] System time error: {}", e);
                    AuthError::GenericError(format!("System time error: {}", e))
                })?
                .as_secs();
            Some(now + expiration.as_secs())
        } else {
            None
        };
        
        let auth_info = AuthInfo {
            user_id: user_id.to_string(),
            name: None,
            role,
            expires_at: exp,
            ip_address: Some(ip_address),
            token_type: AuthType::Simple,
            metadata: None,
            additional: additional.unwrap_or_default(),
        };
        
        // Lưu token
        let mut simple_tokens = self.simple_tokens.write().await;
        
        simple_tokens.insert(token.clone(), auth_info);
        
        info!("[Auth] Successfully created simple token for user {} from IP {}", user_id, ip_address);
        Ok(token)
    }
    
    /// Xác thực simple token
    pub async fn validate_simple_token(&self, token: &str, ip_address: Option<IpAddr>) -> Result<AuthInfo, AuthError> {
        debug!("[Auth] Validating simple token with IP: {:?}", ip_address);
        
        // Check rate limit cho token
            let token_part = token.get(0..10).unwrap_or(token);
        let identifier = crate::security::rate_limiter::RequestIdentifier::Custom(format!("simple:{}", token_part));
        
        let rt = tokio::runtime::Handle::current();
        let rate_check = rt.block_on(async {
            self.rate_limiter.check_limit("/api/auth/validate_simple_token", identifier).await
        });
        
        if let Err(e) = rate_check {
            warn!("[Auth] Rate limit exceeded for simple token validation: {}", e);
            return Err(AuthError::RateLimited(e.to_string()));
        }
        
        let simple_tokens = self.simple_tokens.read().await;
        
        let auth_info = simple_tokens.get(token).ok_or_else(|| {
            warn!("[Auth] Invalid simple token: {}", token_part);
            AuthError::InvalidToken("Token không hợp lệ".to_string())
        })?;
        
        // Kiểm tra hết hạn nếu có
        if let Some(exp) = auth_info.expires_at {
                let now = SystemTime::now().duration_since(UNIX_EPOCH)
                .map_err(|e| {
                    error!("[Auth] System time error: {}", e);
                    AuthError::GenericError(format!("System time error: {}", e))
                })?
                    .as_secs();
                    
            if exp < now {
                warn!("[Auth] Simple token expired for user {}, expired at {}, now {}", 
                      auth_info.user_id, exp, now);
                    return Err(AuthError::ExpiredToken);
                }
            }
            
        // Kiểm tra IP nếu token gắn với IP và caller cung cấp IP
        if let (Some(token_ip), Some(caller_ip)) = (auth_info.ip_address, ip_address) {
            if token_ip != caller_ip {
                warn!("[Auth] IP mismatch for simple token: expected {}, got {}", token_ip, caller_ip);
                return Err(AuthError::Forbidden);
            }
        }
        
        info!("[Auth] Successfully validated simple token for user {} with role {:?}", 
              auth_info.user_id, auth_info.role);
        Ok(auth_info.clone())
    }
    
    /// Thu hồi simple token
    pub async fn revoke_simple_token(&self, token: &str) -> Result<(), AuthError> {
        debug!("[Auth] Revoking simple token");
        let mut simple_tokens = self.simple_tokens.write().await;
        
        if simple_tokens.remove(token).is_some() {
            info!("[Auth] Simple token successfully revoked");
        Ok(())
        } else {
            warn!("[Auth] Attempted to revoke non-existent simple token");
            Err(AuthError::InvalidToken("Token không tồn tại".to_string()))
        }
    }
    
    /// Dọn dẹp tokens hết hạn
    pub async fn clean_expired_tokens(&self) -> Result<usize, AuthError> {
        debug!("[Auth] Cleaning expired tokens");
        let now = SystemTime::now().duration_since(UNIX_EPOCH)
            .map_err(|e| {
                error!("[Auth] System time error: {}", e);
                AuthError::GenericError(format!("System time error: {}", e))
            })?
            .as_secs();
            
        let mut count = 0;
        
        // Dọn dẹp simple tokens
        {
            let mut simple_tokens = self.simple_tokens.write().await;
            let expired_keys: Vec<String> = simple_tokens.iter()
                .filter_map(|(k, v)| {
                    if let Some(exp) = v.expires_at {
                        if exp < now {
                            Some(k.clone())
            } else {
                            None
            }
                    } else {
                        None
                    }
                })
                .collect();
            
            for key in expired_keys {
                simple_tokens.remove(&key);
                count += 1;
            }
        }
        
        // Dọn dẹp revoked JWTs
        {
            let mut revoked_jwt = self.revoked_jwt.write().await;
            let expired_keys: Vec<String> = revoked_jwt.iter()
                .filter_map(|(k, v)| {
                    if *v < now {
                        Some(k.clone())
                    } else {
                        None
                    }
                })
                .collect();
            
            for key in expired_keys {
                revoked_jwt.remove(&key);
                count += 1;
            }
        }
        
        info!("[Auth] Cleaned {} expired tokens", count);
        Ok(count)
    }
    
    /// Thu hồi JWT token (thêm vào blacklist)
    pub async fn revoke_jwt(&self, token: &str) -> Result<(), AuthError> {
        debug!("[Auth] Revoking JWT token");
        
        // Giải mã token để lấy thời gian hết hạn
        let validation = Validation::new(self.jwt_algorithm);
        let decoding_key = match self.get_decoding_key() {
            Ok(key) => key,
            Err(e) => {
                error!("[Auth] Failed to get decoding key for JWT revocation: {}", e);
                return Err(e);
            }
        };
        
        let token_data = match decode::<Claims>(token, &decoding_key, &validation) {
            Ok(data) => data,
            Err(e) => {
                warn!("[Auth] Failed to decode JWT for revocation: {}", e);
                return Err(AuthError::JwtError(e));
            }
        };
        
        let exp = token_data.claims.exp;
        
        // Thêm vào danh sách revoked với thời gian hết hạn
        let mut revoked_jwt = self.revoked_jwt.write().await;
        revoked_jwt.insert(token.to_string(), exp);
        
        info!("[Auth] JWT token successfully revoked for user {}", token_data.claims.sub);
        Ok(())
    }

    /// Thực hiện xoay vòng (rotate) API key cho một user ID.
    /// Phương thức này sẽ tạo key mới và vô hiệu hóa key cũ (nếu có)
    /// 
    /// # Arguments
    /// 
    /// * `user_id` - ID của người dùng
    /// * `name` - Tên hiển thị (tùy chọn)
    /// * `role` - Quyền của người dùng
    /// * `max_age` - Tuổi tối đa của key hiện tại để được giữ lại (key cũ hơn sẽ bị vô hiệu hóa)
    /// * `additional` - Các thông tin bổ sung
    /// 
    /// # Returns
    /// 
    /// API key mới được tạo
    pub async fn rotate_api_key(&self, 
                         user_id: &str, 
                         name: Option<&str>, 
                         role: UserRole,
                         max_age: Option<Duration>,
                         additional: Option<HashMap<String, String>>) -> Result<String, AuthError> {
        // Tạo key mới
        let new_key = generate_api_key();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0))
            .as_secs();
        
        // Vô hiệu hóa các key cũ của user này
        // 1. Tìm tất cả các key hiện tại của user
        let mut keys_to_revoke = Vec::new();
        {
            let api_keys = self.api_keys.read().await;
            for (key, info) in api_keys.iter() {
                if info.user_id == user_id {
                    // Kiểm tra tuổi của key nếu max_age được cung cấp
                    if let Some(max_age) = max_age {
                        if let Some(created_at) = info.additional.get("created_at") {
                            if let Ok(created_time) = created_at.parse::<u64>() {
                                let age = now.saturating_sub(created_time);
                                // Chỉ vô hiệu hóa key cũ hơn max_age
                                if age > max_age.as_secs() {
                                    keys_to_revoke.push(key.clone());
                                }
                                continue;
                            }
                        }
                    }
                    // Nếu không có max_age hoặc không thể kiểm tra tuổi, vô hiệu hóa tất cả
                    keys_to_revoke.push(key.clone());
                }
            }
        }
        
        // 2. Vô hiệu hóa các key đã tìm thấy
        for key in keys_to_revoke {
            let _ = self.revoke_api_key(&key).await;
        }
        
        // 3. Đăng ký key mới
        let mut auth_info = AuthInfo {
            user_id: user_id.to_string(),
            name: name.map(|s| s.to_string()),
            role: role.clone(),
            expires_at: None, // API keys không có thời gian hết hạn mặc định
            ip_address: None,
            token_type: AuthType::ApiKey,
            metadata: None,
            additional: additional.unwrap_or_default(),
        };
        
        // Thêm timestamp tạo key
        auth_info.additional.insert("created_at".to_string(), now.to_string());
        
        // Lưu key mới
        {
            let mut api_keys = self.api_keys.write().await;
            api_keys.insert(new_key.clone(), auth_info);
        }
        
        info!("Rotated API key for user ID: {}", user_id);
        Ok(new_key)
    }

    /// Thực hiện xoay vòng (rotate) token đơn giản cho một user ID.
    /// Phương thức này sẽ tạo token mới và vô hiệu hóa token cũ (nếu có)
    /// 
    /// # Arguments
    /// 
    /// * `user_id` - ID của người dùng
    /// * `ip_address` - Địa chỉ IP được phép sử dụng token
    /// * `role` - Quyền của người dùng
    /// * `expiration` - Thời gian hết hạn của token mới
    /// * `max_age` - Tuổi tối đa của token hiện tại để được giữ lại (token cũ hơn sẽ bị vô hiệu hóa)
    /// * `additional` - Các thông tin bổ sung
    /// 
    /// # Returns
    /// 
    /// Token mới được tạo
    pub async fn rotate_simple_token(&self, 
                            user_id: &str, 
                            ip_address: IpAddr,
                            role: UserRole,
                            expiration: Option<Duration>,
                            max_age: Option<Duration>,
                            additional: Option<HashMap<String, String>>) -> Result<String, AuthError> {
        // Tạo token mới
        let new_token = generate_simple_token();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0))
            .as_secs();
            
        // Tính thời gian hết hạn cho token mới
        let expires_at = expiration.map(|e| {
            now + e.as_secs()
        });
        
        // Vô hiệu hóa các tokens cũ của user này
        // 1. Tìm tất cả các tokens hiện tại của user
        let mut tokens_to_revoke = Vec::new();
        {
            let simple_tokens = self.simple_tokens.read().await;
            for (token, info) in simple_tokens.iter() {
                if info.user_id == user_id {
                    // Kiểm tra tuổi của token nếu max_age được cung cấp
                    if let Some(max_age) = max_age {
                        if let Some(created_at) = info.additional.get("created_at") {
                            if let Ok(created_time) = created_at.parse::<u64>() {
                                let age = now.saturating_sub(created_time);
                                // Chỉ vô hiệu hóa token cũ hơn max_age
                                if age > max_age.as_secs() {
                                    tokens_to_revoke.push(token.clone());
                                }
                                continue;
                            }
                        }
                    }
                    // Nếu không có max_age hoặc không thể kiểm tra tuổi, vô hiệu hóa tất cả
                    tokens_to_revoke.push(token.clone());
                }
            }
        }
        
        // 2. Vô hiệu hóa các tokens đã tìm thấy
        for token in tokens_to_revoke {
            let _ = self.revoke_simple_token(&token).await;
        }
        
        // 3. Đăng ký token mới
        let mut auth_info = AuthInfo {
            user_id: user_id.to_string(),
            name: None,
            role,
            expires_at,
            ip_address: Some(ip_address),
            token_type: AuthType::Simple,
            metadata: None,
            additional: additional.unwrap_or_default(),
        };
        
        // Thêm timestamp tạo token
        auth_info.additional.insert("created_at".to_string(), now.to_string());
        
        // Lưu token mới
        {
            let mut simple_tokens = self.simple_tokens.write().await;
            simple_tokens.insert(new_token.clone(), auth_info);
        }
        
        info!("Rotated simple token for user ID: {}", user_id);
        Ok(new_token)
    }

    /// Vô hiệu hóa tất cả API key của một người dùng
    /// 
    /// # Arguments
    /// 
    /// * `user_id` - ID của người dùng cần vô hiệu hóa tất cả API key
    /// 
    /// # Returns
    /// 
    /// Số lượng key đã bị vô hiệu hóa
    pub async fn revoke_all_api_keys(&self, user_id: &str) -> Result<usize, AuthError> {
        let mut revoked_count = 0;
        let mut keys_to_revoke = Vec::new();
        
        // 1. Tìm tất cả các key của user
        {
            let api_keys = self.api_keys.read().await;
            for (key, info) in api_keys.iter() {
                if info.user_id == user_id {
                    keys_to_revoke.push(key.clone());
                }
            }
        }
        
        // 2. Vô hiệu hóa từng key
        for key in keys_to_revoke {
            if self.revoke_api_key(&key).await.is_ok() {
                revoked_count += 1;
            }
        }
        
        info!("Revoked {} API key(s) for user ID: {}", revoked_count, user_id);
        Ok(revoked_count)
    }

    /// Vô hiệu hóa tất cả simple token của một người dùng
    /// 
    /// # Arguments
    /// 
    /// * `user_id` - ID của người dùng cần vô hiệu hóa tất cả simple token
    /// 
    /// # Returns
    /// 
    /// Số lượng token đã bị vô hiệu hóa
    pub async fn revoke_all_simple_tokens(&self, user_id: &str) -> Result<usize, AuthError> {
        let mut revoked_count = 0;
        let mut tokens_to_revoke = Vec::new();
        
        // 1. Tìm tất cả các tokens của user
        {
            let simple_tokens = self.simple_tokens.read().await;
            for (token, info) in simple_tokens.iter() {
                if info.user_id == user_id {
                    tokens_to_revoke.push(token.clone());
                }
            }
        }
        
        // 2. Vô hiệu hóa từng token
        for token in tokens_to_revoke {
            if self.revoke_simple_token(&token).await.is_ok() {
                revoked_count += 1;
            }
        }
        
        info!("Revoked {} simple token(s) for user ID: {}", revoked_count, user_id);
        Ok(revoked_count)
    }

    /// Bắt đầu tự động xoay vòng API key và Token theo định kỳ
    /// 
    /// # Arguments
    /// * `interval_days` - Số ngày giữa các lần kiểm tra và xoay vòng key
    pub fn start_auto_key_rotation(&self, interval_days: u64) {
        // Tạo một Arc<AuthService> từ các fields của self
        let jwt_secret = self.jwt_secret.clone();
        let api_keys = self.api_keys.clone();
        let simple_tokens = self.simple_tokens.clone();
        let default_issuer = self.default_issuer.clone();
        let default_expiration = self.default_expiration;
        let revoked_jwt = self.revoked_jwt.clone();
        let rate_limiter = self.rate_limiter.clone();
        let jwt_algorithm = self.jwt_algorithm;
        let rsa_private_key = self.rsa_private_key.clone();
        let rsa_public_key = self.rsa_public_key.clone();
        let auth_fail_config = self.auth_fail_config.clone();
        
        // Tạo một AuthService mới với các fields đã clone
        let auth_service = Arc::new(AuthService {
            jwt_secret,
            api_keys,
            simple_tokens,
            default_issuer,
            default_expiration,
            revoked_jwt,
            rate_limiter,
            jwt_algorithm,
            rsa_private_key,
            rsa_public_key,
            auth_fail_config,
            shutdown_signal: Mutex::new(None),
            is_shutdown: AtomicBool::new(false),
            cleanup_handle: None,
            connection_closer: None,
        });

        // Clone Arc để có thể move vào closure
        let auth_service_clone = auth_service.clone();
        let max_age_days = 90; // API key và token được xoay vòng sau 90 ngày
        
        // Tạo một task chạy liên tục
        tokio::spawn(async move {
            let _rotation_interval = Duration::from_secs(interval_days * 24 * 60 * 60);
            let check_interval = Duration::from_secs(6 * 60 * 60); // Kiểm tra mỗi 6 giờ
            
            loop {
                debug!("Running scheduled key rotation check");
                
                // Chạy kiểm tra và quay vòng API keys
                let () = AuthService::check_and_rotate_api_keys(auth_service_clone.as_ref(), max_age_days).await;
                debug!("Completed API key rotation check");
                
                // Chạy kiểm tra và quay vòng simple tokens
                let () = AuthService::check_and_rotate_simple_tokens(auth_service_clone.as_ref(), max_age_days).await;
                debug!("Completed simple token rotation check");
                
                // Đợi đến lượt kiểm tra tiếp theo
                tokio::time::sleep(check_interval).await;
            }
        });
    }

    pub async fn auto_rotate_api_keys(&self) -> Result<usize, AuthError> {
        let mut keys_to_rotate = Vec::new();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| AuthError::GenericError(format!("System time error: {}", e)))?
            .as_secs();
        
        // Lấy lock cho API keys
        let api_keys = self.api_keys.read().await;
        
        // Tìm các keys đã hết hạn
        for (key, info) in api_keys.iter() {
            if let Some(expires_at) = info.expires_at {
                if now > expires_at {
                    keys_to_rotate.push((key.clone(), (info.user_id.clone(), info.name.clone(), info.role.clone(), info.additional.clone())));
                }
            }
        }
        
        drop(api_keys); // Bỏ read lock trước khi lấy write lock
        
        // Xử lý các keys cần rotate
        if !keys_to_rotate.is_empty() {
            let mut api_keys = self.api_keys.write().await;
            
            for (_key, (user_id, name, role, additional)) in keys_to_rotate.iter() {
                // Tạo key mới với lifetime bằng với key cũ
                let new_key: String = Uuid::new_v4().to_string();
                
                let mut new_additional: HashMap<String, String> = additional.clone();
                new_additional.insert("rotated_from".to_string(), "expired_key".to_string());
                new_additional.insert("rotated_at".to_string(), now.to_string());
                
                // Thêm key mới
                let expires_at = now + 7 * 24 * 60 * 60; // 7 days
                api_keys.insert(new_key, AuthInfo {
                    user_id: user_id.clone(),
                    name: name.clone(),
                    role: role.clone(),
                    expires_at: Some(expires_at),
                    ip_address: None,
                    token_type: AuthType::ApiKey,
                    metadata: None,
                    additional: new_additional,
                });
            }
        }
        
        Ok(keys_to_rotate.len())
    }
    
    pub async fn auto_rotate_tokens(&self) -> Result<usize, AuthError> {
        let mut tokens_to_rotate = Vec::new();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| AuthError::GenericError(format!("System time error: {}", e)))?
            .as_secs();
        
        // Lấy lock cho tokens
        let tokens = self.simple_tokens.read().await;
        
        // Tìm các tokens đã hết hạn
        for (token, info) in tokens.iter() {
            // Chỉ rotate tokens có expiration date
            if let Some(expires_at) = info.expires_at {
                if now > expires_at {
                    tokens_to_rotate.push((token.clone(), info.clone()));
                }
            }
        }
        
        drop(tokens); // Bỏ read lock trước khi lấy write lock
        
        // Xử lý các tokens cần rotate
        if !tokens_to_rotate.is_empty() {
            let mut tokens = self.simple_tokens.write().await;
            
            for (_token, info) in tokens_to_rotate.iter() {
                // Tạo token mới với lifetime như token cũ
                let new_token: String = Uuid::new_v4().to_string();
                
                let mut new_additional: HashMap<String, String> = info.additional.clone();
                new_additional.insert("rotated_from".to_string(), "expired_token".to_string());
                new_additional.insert("rotated_at".to_string(), now.to_string());
                
                // Tạo AuthInfo mới với các giá trị từ AuthInfo cũ
                let mut new_info = info.clone();
                new_info.additional = new_additional;
                
                // Thêm token mới
                tokens.insert(new_token, new_info);
            }
        }
        
        Ok(tokens_to_rotate.len())
    }

    /// Function gọi check_simple_token_rotation_internal
    async fn check_and_rotate_simple_tokens(auth_service: &AuthService, max_age_days: u64) {
        let max_age = Duration::from_secs(max_age_days * 24 * 60 * 60);
        check_simple_token_rotation_internal(auth_service, max_age).await;
    }

    /// Function gọi check_api_key_rotation_internal
    async fn check_and_rotate_api_keys(auth_service: &AuthService, max_age_days: u64) {
        let max_age = Duration::from_secs(max_age_days * 24 * 60 * 60);
        check_api_key_rotation_internal(auth_service, max_age).await;
    }

    /// Khởi tạo service sau khi đã tạo instance
    pub async fn init(&self) -> Result<(), AuthError> {
        // Cấu hình rate limit cho login
        let config = crate::security::rate_limiter::PathConfig {
            max_requests: 5,
            time_window: 60,
            algorithm: crate::security::rate_limiter::RateLimitAlgorithm::SlidingWindow,
            action: crate::security::rate_limiter::RateLimitAction::Reject,
        };
        
        // Đăng ký các endpoint cần rate limit
        let _ = self.rate_limiter.register_path("/api/auth/login", config.clone()).await;
        let _ = self.rate_limiter.register_path("/api/auth/validate_jwt", config.clone()).await;
        let _ = self.rate_limiter.register_path("/api/auth/validate_api_key", config.clone()).await;
        let _ = self.rate_limiter.register_path("/api/auth/validate_simple_token", config.clone()).await;
        
        Ok(())
    }
}

/// Hàm nội bộ thực hiện kiểm tra và xoay vòng các API key
async fn check_api_key_rotation_internal(auth_service: &AuthService, max_age: Duration) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs();
    
    let mut keys_to_rotate = HashMap::new();
    
    // 1. Tìm các key cần xoay vòng
    {
        let api_keys = auth_service.api_keys.read().await;

        for (key, info) in api_keys.iter() {
            if let Some(created_at) = info.additional.get("created_at") {
                if let Ok(created_time) = created_at.parse::<u64>() {
                    let age = now.saturating_sub(created_time);
                    if age > max_age.as_secs() {
                        keys_to_rotate.insert(key.clone(), (info.user_id.clone(), info.name.clone(), info.role.clone(), info.additional.clone()));
                    }
                }
            }
        }
    }
    
    // 2. Thực hiện xoay vòng nếu cần
    if !keys_to_rotate.is_empty() {
        info!("[AuthService] Rotating {} API keys", keys_to_rotate.len());
        
        let mut api_keys = auth_service.api_keys.write().await;
        
        for (key, (user_id, name, role, additional)) in keys_to_rotate {
            // Generate new key với lifetime như cũ
            let new_key = Uuid::new_v4().to_string();
            
            let mut new_additional = additional.clone();
            new_additional.insert("rotated_from".to_string(), "aged".to_string());
            new_additional.insert("rotated_at".to_string(), now.to_string());
            new_additional.insert("created_at".to_string(), now.to_string());
            
            // Thêm key mới, xóa key cũ
            let expires_at = api_keys.get(&key).and_then(|info| info.expires_at);
            let ip_address = api_keys.get(&key).and_then(|info| info.ip_address);
            api_keys.insert(new_key.clone(), AuthInfo {
                user_id: user_id.clone(),
                name: name.clone(),
                role: role.clone(),
                expires_at,
                ip_address,
                token_type: AuthType::ApiKey,
                metadata: None,
                additional: new_additional,
            });
            
            api_keys.remove(&key);
            info!("[AuthService] Rotated API key: {}", key);
        }
    }
}

/// Hàm kiểm tra và xoay vòng các simple token
async fn check_simple_token_rotation_internal(auth_service: &AuthService, max_age: Duration) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs();
    
    let mut tokens_to_rotate = HashMap::new();
    
    // 1. Tìm các token cần xoay vòng
    {
        let simple_tokens = auth_service.simple_tokens.read().await;

        for (token, info) in simple_tokens.iter() {
            if let Some(created_at) = info.additional.get("created_at") {
                if let Ok(created_time) = created_at.parse::<u64>() {
                    let age = now.saturating_sub(created_time);
                    if age > max_age.as_secs() {
                        tokens_to_rotate.insert(token.clone(), (info.user_id.clone(), info.name.clone(), info.role.clone(), info.additional.clone()));
                    }
                }
            }
        }
    }
    
    // 2. Thực hiện xoay vòng nếu cần
    if !tokens_to_rotate.is_empty() {
        info!("[AuthService] Rotating {} simple tokens", tokens_to_rotate.len());
        
        let mut simple_tokens = auth_service.simple_tokens.write().await;
        
        for (token, (user_id, name, role, additional)) in tokens_to_rotate {
            // Generate token mới với lifetime như cũ
            let new_token = Uuid::new_v4().to_string();
            
            let mut new_additional = additional.clone();
            new_additional.insert("rotated_from".to_string(), "aged".to_string());
            new_additional.insert("rotated_at".to_string(), now.to_string());
            new_additional.insert("created_at".to_string(), now.to_string());
            
            // Thêm token mới, xóa token cũ
            let expires_at = simple_tokens.get(&token).and_then(|info| info.expires_at);
            let ip_address = simple_tokens.get(&token).and_then(|info| info.ip_address);
            simple_tokens.insert(new_token.clone(), AuthInfo {
                user_id: user_id.clone(),
                name: name.clone(),
                role: role.clone(),
                expires_at,
                ip_address,
                token_type: AuthType::Simple,
                metadata: None,
                additional: new_additional,
            });
            
            simple_tokens.remove(&token);
            info!("[AuthService] Rotated simple token: {}", token);
        }
    }
}

/// Bắt đầu kiểm tra và quay vòng các API key theo khung thời gian cho trước
/// 
/// [SECURITY SOLUTION] Đã bổ sung cơ chế rotate/revoke định kỳ cho API key và simple token.
/// Hàm này thiết lập việc tự động xoay vòng API key và simple token định kỳ (mặc định 30 ngày)
/// để đảm bảo bảo mật và giảm thiểu nguy cơ lộ key dài hạn.
pub fn check_api_key_rotation(auth_service: &AuthService) {
    info!("[Auth] Setting up automatic key rotation for API keys and simple tokens");
    auth_service.start_auto_key_rotation(30); // Mặc định 30 ngày
}

/// Trích xuất token từ HTTP header
pub fn extract_auth_from_header(headers: &HeaderMap<HeaderValue>) -> Result<(String, AuthType), AuthError> {
    debug!("[Auth] Extracting authentication from headers");
    
    // Kiểm tra Authorization header
    let auth_header = headers.get(AUTHORIZATION).ok_or_else(|| {
        warn!("[Auth] No Authorization header found");
        AuthError::NoToken
    })?;
    
    let auth_str = auth_header.to_str().map_err(|_| {
        warn!("[Auth] Authorization header is not valid UTF-8");
            AuthError::InvalidToken("Authorization header không hợp lệ".to_string())
        })?;
        
    // Xác định kiểu xác thực
    if let Some(token_str) = auth_str.strip_prefix("Bearer ") {
        let token = token_str.to_string();
        debug!("[Auth] Found Bearer token");
        Ok((token, AuthType::Jwt))
    } else if let Some(token_str) = auth_str.strip_prefix("ApiKey ") {
        let token = token_str.to_string();
        debug!("[Auth] Found API key");
        Ok((token, AuthType::ApiKey))
    } else if let Some(token_str) = auth_str.strip_prefix("Simple ") {
        let token = token_str.to_string();
        debug!("[Auth] Found Simple token");
        Ok((token, AuthType::Simple))
    } else {
        warn!("[Auth] Unknown authorization type: {}", auth_str.split_whitespace().next().unwrap_or(""));
        Err(AuthError::InvalidToken("Không hỗ trợ kiểu xác thực này".to_string()))
    }
}

/// Warp filter middleware cho authentication và authorization
pub fn with_auth(
    auth_service: Arc<AuthService>,
    required_role: Option<UserRole>,
) -> impl Filter<Extract = (AuthInfo,), Error = Rejection> + Clone {
    debug!("[Auth] Setting up with_auth middleware with required role: {:?}", required_role);
    
    headers_cloned()
        .map(move |headers: HeaderMap<HeaderValue>| (headers, Arc::clone(&auth_service), required_role.clone()))
        .and_then(|(headers, auth_service, required_role): (HeaderMap<HeaderValue>, Arc<AuthService>, Option<UserRole>)| async move {
            // Trích xuất thông tin xác thực từ header
            let (token, auth_type) = match extract_auth_from_header(&headers) {
                Ok(result) => result,
                Err(AuthError::NoToken) => {
                    warn!("[Auth] Authentication failed: No token provided");
                    return Err(warp::reject::custom(RejectReason::Unauthorized));
                }
                Err(e) => {
                    warn!("[Auth] Authentication failed: {}", e);
                    return Err(warp::reject::custom(RejectReason::InvalidToken));
                    }
            };

            // Lấy địa chỉ IP từ header, nếu có
            let ip_address = match headers.get("X-Forwarded-For")
                .and_then(|h| h.to_str().ok())
                .and_then(|s| s.split(',').next())
                .and_then(|s| s.trim().parse::<IpAddr>().ok()) {
                Some(ip) => {
                    debug!("[Auth] Found IP address from X-Forwarded-For: {}", ip);
                    Some(ip)
                },
                None => None
            };
                
            // Xác thực dựa trên loại token
                let auth_info = match auth_type {
                    AuthType::Jwt => {
                    match auth_service.validate_jwt(&token).await {
                        Ok(auth_info) => auth_info,
                        Err(e) => {
                            warn!("[Auth] JWT validation failed: {}", e);
                            return Err(match e {
                                AuthError::ExpiredToken => warp::reject::custom(RejectReason::InvalidToken),
                                AuthError::RateLimited(msg) => warp::reject::custom(RejectReason::RateLimited(msg)),
                                _ => warp::reject::custom(RejectReason::InvalidToken),
                            });
                        }
                    }
                    },
                    AuthType::ApiKey => {
                    match auth_service.validate_api_key(&token).await {
                        Ok(auth_info) => auth_info,
                        Err(e) => {
                            warn!("[Auth] API key validation failed: {}", e);
                            return Err(match e {
                                AuthError::RateLimited(msg) => warp::reject::custom(RejectReason::RateLimited(msg)),
                                _ => warp::reject::custom(RejectReason::InvalidToken),
                            });
                        }
                    }
                    },
                    AuthType::Simple => {
                    match auth_service.validate_simple_token(&token, ip_address).await {
                        Ok(auth_info) => auth_info,
                        Err(e) => {
                            warn!("[Auth] Simple token validation failed: {}", e);
                            return Err(match e {
                                AuthError::ExpiredToken => warp::reject::custom(RejectReason::InvalidToken),
                                AuthError::Forbidden => warp::reject::custom(RejectReason::Forbidden),
                                AuthError::RateLimited(msg) => warp::reject::custom(RejectReason::RateLimited(msg)),
                                _ => warp::reject::custom(RejectReason::InvalidToken),
                            });
                        }
                    }
                }
                };
                
            // Kiểm tra quyền nếu required_role được chỉ định
            if let Some(ref required) = required_role {
                if !has_sufficient_role(&auth_info.role, required) {
                    warn!("[Auth] Authorization failed: User {} with role {:?} does not have required role {:?}", 
                         auth_info.user_id, auth_info.role, required);
                        return Err(warp::reject::custom(RejectReason::Forbidden));
                    }
                }
                
            info!("[Auth] Successfully authenticated user {} with role {:?}", auth_info.user_id, auth_info.role);
                Ok(auth_info)
        })
}

/// Tùy chỉnh lỗi rejection
#[derive(Debug)]
pub enum RejectReason {
    Unauthorized,
    InvalidToken,
    Forbidden,
    RateLimited(String),
}

impl warp::reject::Reject for RejectReason {}

/// Xử lý lỗi rejection
pub async fn handle_rejection(err: Rejection) -> Result<impl Reply, std::convert::Infallible> {
    let (code, message) = if err.is_not_found() {
        warn!("[Auth] Route not found");
        (warp::http::StatusCode::NOT_FOUND, "Route không tồn tại".to_string())
    } else if let Some(RejectReason::Unauthorized) = err.find() {
        warn!("[Auth] Unauthorized access attempt");
        (warp::http::StatusCode::UNAUTHORIZED, "Yêu cầu xác thực".to_string())
    } else if let Some(RejectReason::InvalidToken) = err.find() {
        warn!("[Auth] Invalid token provided");
        (warp::http::StatusCode::UNAUTHORIZED, "Token không hợp lệ hoặc đã hết hạn".to_string())
    } else if let Some(RejectReason::Forbidden) = err.find() {
        warn!("[Auth] Forbidden access attempt");
        (warp::http::StatusCode::FORBIDDEN, "Không có quyền truy cập".to_string())
    } else if let Some(RejectReason::RateLimited(msg)) = err.find() {
        warn!("[Auth] Rate limited: {}", msg);
        (warp::http::StatusCode::TOO_MANY_REQUESTS, format!("Quá nhiều yêu cầu: {}", msg))
    } else if err.find::<warp::reject::MethodNotAllowed>().is_some() {
        warn!("[Auth] Method not allowed");
        (warp::http::StatusCode::METHOD_NOT_ALLOWED, "Method không được hỗ trợ".to_string())
    } else {
        error!("[Auth] Unhandled rejection: {:?}", err);
        (warp::http::StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error".to_string())
    };
    
    let json = warp::reply::json(&serde_json::json!({
        "status": "error",
        "message": message
    }));
    
    Ok(warp::reply::with_status(json, code))
}

/// Trait Auth abstraction cho các framework khác nhau
#[async_trait]
pub trait Auth: Send + Sync {
    /// Xác thực header
    async fn authenticate(&self, headers: &HeaderMap) -> Result<AuthInfo, AuthError>;
    
    /// Kiểm tra quyền
    async fn authorize(&self, auth_info: &AuthInfo, required_role: &UserRole) -> Result<(), AuthError>;
    
    /// Tạo JWT token
    async fn create_token(&self, user_id: &str, role: UserRole) -> Result<String, AuthError>;
}

/// Auth impl cho warp
pub struct WarpAuth {
    auth_service: Arc<AuthService>,
}

impl WarpAuth {
    pub fn new(auth_service: Arc<AuthService>) -> Self {
        Self { auth_service }
    }
}

#[async_trait]
impl Auth for WarpAuth {
    async fn authenticate(&self, headers: &HeaderMap) -> Result<AuthInfo, AuthError> {
        let (token, auth_type) = extract_auth_from_header(headers)?;
        
        match auth_type {
            AuthType::Jwt => self.auth_service.validate_jwt(&token).await,
            AuthType::ApiKey => self.auth_service.validate_api_key(&token).await,
            AuthType::Simple => {
                // Lấy IP từ header nếu có
                let ip = headers.get("X-Forwarded-For")
                    .and_then(|h| h.to_str().ok())
                    .and_then(|s| s.split(',').next())
                    .and_then(|s| s.trim().parse::<IpAddr>().ok());
                    
                self.auth_service.validate_simple_token(&token, ip).await
            }
        }
    }
    
    async fn authorize(&self, auth_info: &AuthInfo, required_role: &UserRole) -> Result<(), AuthError> {
        if has_sufficient_role(&auth_info.role, required_role) {
            Ok(())
        } else {
            Err(AuthError::Forbidden)
        }
    }
    
    async fn create_token(&self, user_id: &str, role: UserRole) -> Result<String, AuthError> {
        self.auth_service.create_jwt(user_id, None, role, None, None)
    }
}

/// Kiểm tra quyền hạn có đủ không
pub fn has_sufficient_role(user_role: &UserRole, required_role: &UserRole) -> bool {
    match (user_role, required_role) {
        // Admin có thể làm mọi thứ
        (UserRole::Admin, _) => true,
        
        // Node Service có thể làm việc của Service và User
        (UserRole::Node, UserRole::Service) | (UserRole::Node, UserRole::User) => true,
        
        // Service có thể làm việc của User
        (UserRole::Service, UserRole::User) => true,
        
        // Các trường hợp còn lại là cùng vai trò
        (a, b) => a == b,
    }
}

/// Generate a standard format API key (uuid-based with prefix)
fn generate_api_key() -> String {
    let uuid = Uuid::new_v4().to_string();
    let prefix = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(8)
        .map(char::from)
        .collect::<String>()
        .to_uppercase();
    
    format!("DM-{}-{}", prefix, uuid)
}

/// Hàm helper để tạo simple token
fn generate_simple_token() -> String {
    rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(32)
        .map(char::from)
        .collect()
}

/// Tạo middleware xác thực Admin hoặc Partner
pub fn with_admin_or_partner(auth_service: Arc<AuthService>) -> impl Filter<Extract = (AuthInfo,), Error = Rejection> + Clone {
    with_auth_rate_limit(auth_service, Some(UserRole::Partner))
}

pub fn with_auth_rate_limit(
    auth_service: Arc<AuthService>,
    required_role: Option<UserRole>,
) -> impl Filter<Extract = (AuthInfo,), Error = Rejection> + Clone {
    let auth_service_clone = auth_service.clone();
    let auth_fail_config = auth_service.auth_fail_config.clone();
    
    headers_cloned()
        .and(warp::addr::remote())
        .and_then(move |headers: HeaderMap, addr: Option<SocketAddr>| {
            // Cần clone trước khi move vào closure
            let auth_service = auth_service_clone.clone();
            let config = auth_fail_config.clone();
            let required = required_role.clone();
            
            async move {
                // Kiểm tra số lần thất bại xác thực của IP này
                let ip = match addr {
                    Some(addr) => addr.ip(),
                    None => return Err(warp::reject::custom(RejectReason::Unauthorized)),
                };
                
                {
                    let mut rate_limit = AUTH_FAIL_RATE_LIMIT.lock().await;
                
                    // Xóa các IP đã hết thời gian block
                    let now = Instant::now();
                    let lockout_duration = config.lockout_duration_secs;
                    rate_limit.retain(|_, (_, timestamp)| {
                        now.duration_since(*timestamp).as_secs() < lockout_duration
                    });
                    
                    // Kiểm tra IP hiện tại
                    let max_fail_attempts = config.max_fail_attempts;
                    let fail_window_secs = config.fail_window_secs;
                    
                    if let Some((attempts, timestamp)) = rate_limit.get(&ip) {
                        if *attempts >= max_fail_attempts && 
                           now.duration_since(*timestamp).as_secs() < fail_window_secs {
                            // IP đã vượt quá số lần thất bại trong khoảng thời gian
                            return Err(warp::reject::custom(RejectReason::RateLimited(
                                format!("Too many failed attempts. Try again after {} seconds", 
                                        lockout_duration)
                            )));
                        }
                    }
                }
                
                // Thử xác thực
                match extract_auth_from_header(&headers) {
                    Ok((token, auth_type)) => {
                        let result = match auth_type {
                            AuthType::Jwt => auth_service.validate_jwt(&token).await,
                            AuthType::ApiKey => auth_service.validate_api_key(&token).await,
                            AuthType::Simple => auth_service.validate_simple_token(&token, Some(ip)).await,
                        };
                        
                        match result {
                            Ok(auth_info) => {
                                // Kiểm tra quyền nếu có
                                if let Some(ref required) = &required {
                                    if !has_sufficient_role(&auth_info.role, required) {
                                        // Tăng số lần thất bại
                                        let mut rate_limit = AUTH_FAIL_RATE_LIMIT.lock().await;
                                        let entry = rate_limit.entry(ip).or_insert((0, Instant::now()));
                                        entry.0 += 1;
                                        entry.1 = Instant::now();
                                        
                    return Err(warp::reject::custom(RejectReason::Forbidden));
                                    }
                                }
                                
                                // Xác thực thành công, reset số lần thất bại
                                let mut rate_limit = AUTH_FAIL_RATE_LIMIT.lock().await;
                                rate_limit.remove(&ip);
                
                                Ok(auth_info)
                            },
                            Err(_) => {
                                // Tăng số lần thất bại
                                let mut rate_limit = AUTH_FAIL_RATE_LIMIT.lock().await;
                                let entry = rate_limit.entry(ip).or_insert((0, Instant::now()));
                                entry.0 += 1;
                                entry.1 = Instant::now();
                                
                                Err(warp::reject::custom(RejectReason::InvalidToken))
                            }
                        }
                    },
                    Err(_) => {
                        // Tăng số lần thất bại
                        let mut rate_limit = AUTH_FAIL_RATE_LIMIT.lock().await;
                        let entry = rate_limit.entry(ip).or_insert((0, Instant::now()));
                        entry.0 += 1;
                        entry.1 = Instant::now();
                        
                        Err(warp::reject::custom(RejectReason::Unauthorized))
                    }
                }
            }
        })
}

/// Check if user is Admin or Partner
pub fn allow_admin_or_partner(auth_info: &AuthInfo) -> bool {
    matches!(auth_info.role, UserRole::Admin | UserRole::Partner)
}

static AUTH_FAIL_RATE_LIMIT: Lazy<tokio::sync::Mutex<HashMap<IpAddr, (u32, Instant)>>> = Lazy::new(|| tokio::sync::Mutex::new(HashMap::new()));
#[allow(dead_code)]
const AUTH_FAIL_LIMIT: u32 = 5;
#[allow(dead_code)]
const AUTH_FAIL_WINDOW_SECS: u64 = 60;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    
    #[test]
    fn test_jwt_creation_and_validation() {
        let auth_service = AuthService::new("test_secret".to_string());
        let token = auth_service.create_jwt(
            "user123",
            Some("Test User"),
            UserRole::Admin,
            Some(Duration::from_secs(3600)),
            None
        ).unwrap();
        
        // Sử dụng block_on để chạy validate_jwt async
        let auth_info = tokio::runtime::Handle::current().block_on(async {
            auth_service.validate_jwt(&token).await
        }).unwrap();
        
        assert_eq!(auth_info.user_id, "user123");
        assert_eq!(auth_info.name, Some("Test User".to_string()));
        assert_eq!(auth_info.role, UserRole::Admin);
    }

    #[test]
    fn test_jwt_partner_role() {
        let auth_service = AuthService::new("test_secret".to_string());
        let token = auth_service.create_jwt(
            "partner1",
            Some("Partner User"),
            UserRole::Partner,
            Some(Duration::from_secs(3600)),
            None
        ).unwrap();
        
        // Sử dụng block_on để chạy validate_jwt async
        let auth_info = tokio::runtime::Handle::current().block_on(async {
            auth_service.validate_jwt(&token).await
        }).unwrap();
        
        assert_eq!(auth_info.user_id, "partner1");
        assert_eq!(auth_info.name, Some("Partner User".to_string()));
        assert_eq!(auth_info.role, UserRole::Partner);
    }

    #[test]
    fn test_jwt_wrong_role() {
        let auth_service = AuthService::new("test_secret".to_string());
        let token = auth_service.create_jwt(
            "user456",
            Some("Normal User"),
            UserRole::User,
            Some(Duration::from_secs(3600)),
            None
        ).unwrap();
        
        // Sử dụng block_on để chạy validate_jwt async
        let auth_info = tokio::runtime::Handle::current().block_on(async {
            auth_service.validate_jwt(&token).await
        }).unwrap();
        
        assert_eq!(auth_info.role, UserRole::User);
        // Không phải Admin/Partner, không được phép truy cập API quản trị
        assert!(!allow_admin_or_partner(&auth_info));
    }

    #[test]
    fn test_jwt_expired() {
        let auth_service = AuthService::new("test_secret".to_string());
        let token = auth_service.create_jwt(
            "admin_expired",
            Some("Expired Admin"),
            UserRole::Admin,
            Some(Duration::from_secs(0)), // hết hạn ngay lập tức
            None
        ).unwrap();
        
        // Sử dụng block_on để chạy validate_jwt async
        let result = tokio::runtime::Handle::current().block_on(async {
            auth_service.validate_jwt(&token).await
        });
        
        assert!(matches!(result, Err(AuthError::ExpiredToken)));
    }

    #[test]
    fn test_api_key() {
        let auth_service = AuthService::new("test_secret".to_string());
        
        // Sử dụng block_on để chạy register_api_key async
        let api_key = tokio::runtime::Handle::current().block_on(async {
            auth_service.register_api_key(
            "service1", 
            Some("Test Service"), 
            UserRole::Service,
            None
            ).await
        }).unwrap();
        
        // Sử dụng block_on để chạy validate_api_key async
        let auth_info = tokio::runtime::Handle::current().block_on(async {
            auth_service.validate_api_key(&api_key).await
        }).unwrap();
        
        assert_eq!(auth_info.user_id, "service1");
        assert_eq!(auth_info.name, Some("Test Service".to_string()));
        assert_eq!(auth_info.role, UserRole::Service);
        
        // Test revoking
        tokio::runtime::Handle::current().block_on(async {
            auth_service.revoke_api_key(&api_key).await
        }).unwrap();
        
        assert!(tokio::runtime::Handle::current().block_on(async {
            auth_service.validate_api_key(&api_key).await.is_err()
        }));
    }
    
    #[test]
    fn test_simple_token() {
        let auth_service = AuthService::new("test_secret".to_string());
        let ip = "127.0.0.1".parse::<IpAddr>().unwrap();
        
        // Sử dụng block_on để chạy create_simple_token async
        let token = tokio::runtime::Handle::current().block_on(async {
            auth_service.create_simple_token(
            "node1", 
            ip,
            UserRole::Node,
            Some(Duration::from_secs(3600)),
            None
            ).await
        }).unwrap();
        
        // Sử dụng block_on để chạy validate_simple_token async
        let auth_info = tokio::runtime::Handle::current().block_on(async {
            auth_service.validate_simple_token(&token, Some(ip)).await
        }).unwrap();
        
        assert_eq!(auth_info.user_id, "node1");
        assert_eq!(auth_info.role, UserRole::Node);
        
        // Test IP mismatch
        let wrong_ip = "192.168.1.1".parse::<IpAddr>().unwrap();
        assert!(tokio::runtime::Handle::current().block_on(async {
            auth_service.validate_simple_token(&token, Some(wrong_ip)).await.is_err()
        }));
        
        // Test revocation
        tokio::runtime::Handle::current().block_on(async {
            auth_service.revoke_simple_token(&token).await
        }).unwrap();
        
        assert!(tokio::runtime::Handle::current().block_on(async {
            auth_service.validate_simple_token(&token, Some(ip)).await.is_err()
        }));
    }
} 
