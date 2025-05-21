//! # JWT Service
//! 
//! Module này cung cấp các dịch vụ JWT cho authentication và authorization.
//!
//! ## Mối quan hệ với AuthService
//!
//! Module này được thiết kế để sử dụng độc lập hoặc kết hợp với AuthService trong auth_middleware.rs:
//! - Khi cần xử lý JWT đơn giản, không cần đầy đủ tính năng của AuthService, có thể dùng JwtService trực tiếp
//! - AuthService sử dụng các tính năng tương tự nhưng thêm các logic kiểm tra quyền và quản lý token phức tạp hơn
//! - Lưu ý: KHÔNG nên sử dụng cả hai service cho cùng một endpoint để tránh xung đột

use jsonwebtoken::{encode, decode, Header, EncodingKey, DecodingKey, Validation, Algorithm};
use serde::{Serialize, Deserialize};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::security::auth_middleware::AuthError;
use std::sync::Arc;
use std::fs;

/// Cấu hình cho JWT
#[derive(Debug, Clone)]
pub struct JwtConfig {
    pub algorithm: Algorithm,
    pub secret: String,
    pub expiry_seconds: u64,
    pub issuer: Option<String>,
    pub audience: Option<String>,
    pub rsa_private_key_path: Option<String>,
    pub rsa_public_key_path: Option<String>,
}

impl Default for JwtConfig {
    fn default() -> Self {
        Self {
            algorithm: Algorithm::HS256,
            secret: "default_jwt_secret_for_development_only".to_string(),
            expiry_seconds: 3600,
            issuer: None,
            audience: None,
            rsa_private_key_path: None,
            rsa_public_key_path: None,
        }
    }
}

/// Claims cho JWT
#[derive(Debug, Serialize, Deserialize)]
pub struct JwtClaims {
    pub sub: String,
    pub exp: u64,
    pub iat: u64,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aud: Option<String>,
    #[serde(flatten)]
    pub additional: Option<serde_json::Value>,
}

/// JWT Service
#[derive(Clone)]
pub struct JwtService {
    config: Arc<JwtConfig>,
    encoding_key: Option<EncodingKey>,
    decoding_key: Option<DecodingKey>,
}

impl JwtService {
    /// Tạo JwtService mới
    pub fn new(config: JwtConfig) -> Result<Self, AuthError> {
        let mut service = Self {
            config: Arc::new(config),
            encoding_key: None,
            decoding_key: None,
        };
        
        service.init_keys()?;
        Ok(service)
    }
    
    /// Khởi tạo các keys
    fn init_keys(&mut self) -> Result<(), AuthError> {
        match self.config.algorithm {
            Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512 => {
                self.encoding_key = Some(EncodingKey::from_secret(self.config.secret.as_bytes()));
                self.decoding_key = Some(DecodingKey::from_secret(self.config.secret.as_bytes()));
            },
            Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512 => {
                // Đọc private key từ file
                if let Some(private_key_path) = &self.config.rsa_private_key_path {
                    match fs::read_to_string(private_key_path) {
                        Ok(private_key) => {
                            self.encoding_key = Some(
                                EncodingKey::from_rsa_pem(private_key.as_bytes())
                                    .map_err(|e| AuthError::GenericError(format!("Invalid RSA private key: {}", e)))?
                            );
                        },
                        Err(e) => {
                            return Err(AuthError::GenericError(format!("Could not read RSA private key file: {}", e)));
                        }
                    }
                }
                
                // Đọc public key từ file
                if let Some(public_key_path) = &self.config.rsa_public_key_path {
                    match fs::read_to_string(public_key_path) {
                        Ok(public_key) => {
                            self.decoding_key = Some(
                                DecodingKey::from_rsa_pem(public_key.as_bytes())
                                    .map_err(|e| AuthError::GenericError(format!("Invalid RSA public key: {}", e)))?
                            );
                        },
                        Err(e) => {
                            return Err(AuthError::GenericError(format!("Could not read RSA public key file: {}", e)));
                        }
                    }
                }
            },
            _ => {
                return Err(AuthError::GenericError(format!("Unsupported JWT algorithm: {:?}", self.config.algorithm)));
            }
        }
        
        Ok(())
    }
    
    /// Tạo JWT token
    pub fn create_token(
        &self,
        subject: &str,
        role: &str,
        additional_claims: Option<serde_json::Value>,
    ) -> Result<String, AuthError> {
        let encoding_key = self.encoding_key.as_ref()
            .ok_or_else(|| AuthError::GenericError("JWT encoding key not initialized".to_string()))?;
        
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| AuthError::GenericError(format!("System time error: {}", e)))?
            .as_secs();
        
        let expiry = now + self.config.expiry_seconds;
        
        let claims = JwtClaims {
            sub: subject.to_string(),
            exp: expiry,
            iat: now,
            role: role.to_string(),
            iss: self.config.issuer.clone(),
            aud: self.config.audience.clone(),
            additional: additional_claims,
        };
        
        encode(
            &Header::new(self.config.algorithm),
            &claims,
            encoding_key,
        )
        .map_err(AuthError::JwtError)
    }
    
    /// Xác thực JWT token
    pub fn validate_token(&self, token: &str) -> Result<JwtClaims, AuthError> {
        let decoding_key = self.decoding_key.as_ref()
            .ok_or_else(|| AuthError::GenericError("JWT decoding key not initialized".to_string()))?;
        
        let mut validation = Validation::new(self.config.algorithm);
        
        if let Some(ref issuer) = self.config.issuer {
            validation.set_issuer(&[issuer]);
        }
        
        if let Some(ref audience) = self.config.audience {
            validation.set_audience(&[audience]);
        }
        
        let decoded = decode::<JwtClaims>(
            token,
            decoding_key,
            &validation,
        )
        .map_err(|e| {
            match e.kind() {
                jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::ExpiredToken,
                _ => AuthError::JwtError(e),
            }
        })?;
        
        // Kiểm tra thêm expiry
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| AuthError::GenericError(format!("System time error: {}", e)))?
            .as_secs();
        
        if decoded.claims.exp < now {
            return Err(AuthError::ExpiredToken);
        }
        
        Ok(decoded.claims)
    }
} 