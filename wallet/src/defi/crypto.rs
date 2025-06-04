// External imports
use aes_gcm::{aead::{Aead, KeyInit}, Aes256Gcm, Nonce};
use hmac::Hmac;
use pbkdf2::pbkdf2_hmac;
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};
use anyhow::Result;

// Internal imports
use crate::defi::error::DefiError;

// Hằng số cho thông báo lỗi
const ERR_FAILED_TO_INIT_CIPHER: &str = "Failed to init cipher: {}";
const ERR_ENCRYPTION_FAILED: &str = "Encryption failed: {}";
const ERR_DECRYPTION_FAILED: &str = "Decryption failed: {}";
const ERR_INVALID_UTF8: &str = "Invalid UTF-8 in plaintext: {}";

// Hằng số cho cấu hình bảo mật
const PBKDF2_ITERATIONS: u32 = 100_000;
const SALT_SIZE: usize = 16;
const KEY_SIZE: usize = 32;
const NONCE_SIZE: usize = 12;

/// Version and timestamp để quản lý key rotation
#[derive(Debug, Clone, Copy)]
pub struct KeyMetadata {
    pub version: u32,
    pub created_at: u64,
    pub expires_at: Option<u64>,
}

impl KeyMetadata {
    /// Tạo metadata mới với version hiện tại
    pub fn new(version: u32, expiry_days: Option<u64>) -> Self {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let expires_at = expiry_days.map(|days| now + days * 86400);
        
        Self {
            version,
            created_at: now,
            expires_at,
        }
    }
    
    /// Kiểm tra key có còn hiệu lực không
    pub fn is_valid(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
            now < expires_at
        } else {
            true // Không có expiry = always valid
        }
    }
    
    /// Chuyển metadata thành bytes để lưu trữ
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut result = Vec::with_capacity(16);
        result.extend_from_slice(&self.version.to_be_bytes());
        result.extend_from_slice(&self.created_at.to_be_bytes());
        
        if let Some(expires_at) = self.expires_at {
            result.push(1); // Has expiry
            result.extend_from_slice(&expires_at.to_be_bytes());
        } else {
            result.push(0); // No expiry
        }
        
        result
    }
    
    /// Chuyển từ bytes thành metadata
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DefiError> {
        if bytes.len() < 13 {
            return Err(DefiError::SecurityError(ERR_INVALID_KEY_METADATA.to_string()));
        }
        
        let version = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let created_at = u64::from_be_bytes([
            bytes[4], bytes[5], bytes[6], bytes[7], bytes[8], bytes[9], bytes[10], bytes[11]
        ]);
        
        let has_expiry = bytes[12] == 1;
        let expires_at = if has_expiry && bytes.len() >= 21 {
            Some(u64::from_be_bytes([
                bytes[13], bytes[14], bytes[15], bytes[16], 
                bytes[17], bytes[18], bytes[19], bytes[20]
            ]))
        } else {
            None
        };
        
        Ok(Self {
            version,
            created_at,
            expires_at,
        })
    }
}

/// Mô tả các phiên bản khóa hiện tại và cũ
pub struct KeyRotationManager {
    current_version: u32,
    keys: Vec<(u32, String)>, // (version, key_salt)
    default_expiry_days: Option<u64>,
}

impl KeyRotationManager {
    /// Tạo key manager mới với version đầu tiên
    pub fn new(master_key: &str, default_expiry_days: Option<u64>) -> Self {
        let current_version = 1;
        let keys = vec![(current_version, master_key.to_string())];
        
        Self {
            current_version,
            keys,
            default_expiry_days,
        }
    }
    
    /// Rotate key - tạo version mới và thêm vào danh sách
    pub fn rotate_key(&mut self, new_master_key: &str) {
        self.current_version += 1;
        self.keys.push((self.current_version, new_master_key.to_string()));
    }
    
    /// Lấy key theo version
    pub fn get_key_by_version(&self, version: u32) -> Option<&str> {
        self.keys.iter()
            .find(|(v, _)| *v == version)
            .map(|(_, key)| key.as_str())
    }
    
    /// Lấy key hiện tại
    pub fn get_current_key(&self) -> &str {
        self.keys.iter()
            .find(|(v, _)| *v == self.current_version)
            .map(|(_, key)| key.as_str())
            .unwrap() // Safe because we always have at least one key
    }
    
    /// Lấy metadata cho key hiện tại 
    pub fn get_current_metadata(&self) -> KeyMetadata {
        KeyMetadata::new(self.current_version, self.default_expiry_days)
    }
    
    /// Re-encrypt dữ liệu với key mới
    pub fn reencrypt(
        &self,
        ciphertext: &[u8],
        nonce: &[u8],
        salt: &[u8],
        metadata_bytes: &[u8],
        password: &str,
    ) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>), DefiError> {
        // Đọc metadata từ chuỗi bytes
        let metadata = KeyMetadata::from_bytes(metadata_bytes)?;
        
        // Kiểm tra trạng thái key
        if !metadata.is_valid() {
            return Err(DefiError::SecurityError(ERR_KEY_EXPIRATION.to_string()));
        }
        
        // Tìm key cũ theo version
        let old_key = self.get_key_by_version(metadata.version)
            .ok_or_else(|| {
                DefiError::SecurityError(
                    format!(ERR_KEY_VERSION_MISMATCH, metadata.version, self.current_version)
                )
            })?;
        
        // Giải mã với key cũ
        let plaintext = decrypt_data_with_key(ciphertext, nonce, salt, password, old_key)?;
        
        // Mã hóa lại với key mới
        let current_key = self.get_current_key();
        let current_metadata = self.get_current_metadata();
        
        let (new_ciphertext, new_nonce, new_salt) = encrypt_data_with_key(
            &plaintext, password, current_key
        )?;
        
        Ok((new_ciphertext, new_nonce, new_salt, current_metadata.to_bytes()))
    }
}

/// (Ciphertext, nonce, salt, metadata) mã hóa.
///
/// # Examples
/// ```
/// use wallet::defi::crypto::encrypt_data_with_rotation;
///
/// let data = "sensitive_data";
/// let password = "secure_password";
/// let master_key = "master_key_from_secure_storage";
/// let (ciphertext, nonce, salt, metadata) = encrypt_data_with_rotation(data, password, master_key).unwrap();
/// ```
pub fn encrypt_data_with_rotation(
    data: &str, 
    password: &str,
    master_key: &str
) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>), DefiError> {
    let manager = KeyRotationManager::new(master_key, Some(90)); // 90 days expiry
    let metadata = manager.get_current_metadata();
    let key = manager.get_current_key();
    
    let (ciphertext, nonce, salt) = encrypt_data_with_key(data, password, key)?;
    let metadata_bytes = metadata.to_bytes();
    
    Ok((ciphertext, nonce, salt, metadata_bytes))
}

/// Giải mã dữ liệu với rotation support.
///
/// # Examples
/// ```
/// use wallet::defi::crypto::{encrypt_data_with_rotation, decrypt_data_with_rotation};
///
/// let data = "sensitive_data";
/// let password = "secure_password";
/// let master_key = "master_key_from_secure_storage";
/// let (ciphertext, nonce, salt, metadata) = encrypt_data_with_rotation(data, password, master_key).unwrap();
/// 
/// let decrypted = decrypt_data_with_rotation(&ciphertext, &nonce, &salt, &metadata, password, master_key).unwrap();
/// assert_eq!(data, decrypted);
/// ```
pub fn decrypt_data_with_rotation(
    ciphertext: &[u8],
    nonce: &[u8],
    salt: &[u8],
    metadata_bytes: &[u8],
    password: &str,
    master_key: &str
) -> Result<String, DefiError> {
    let metadata = KeyMetadata::from_bytes(metadata_bytes)?;
    
    // Kiểm tra validity
    if !metadata.is_valid() {
        return Err(DefiError::SecurityError(ERR_KEY_EXPIRATION.to_string()));
    }
    
    let manager = KeyRotationManager::new(master_key, Some(90));
    let key = manager.get_key_by_version(metadata.version)
        .ok_or_else(|| {
            DefiError::SecurityError(
                format!(ERR_KEY_VERSION_MISMATCH, metadata.version, manager.current_version)
            )
        })?;
    
    decrypt_data_with_key(ciphertext, nonce, salt, password, key)
}

/// Encrypt data với custom key derivation function
pub fn encrypt_data_with_key(
    data: &str, 
    password: &str, 
    key_salt: &str
) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>), DefiError> {
    let mut salt = [0u8; SALT_SIZE];
    OsRng.fill_bytes(&mut salt);
    
    // Tạo final key từ password, salt và key
    let derived_key = derive_encryption_key(password, &salt, key_salt)?;
    
    let cipher = Aes256Gcm::new_from_slice(&derived_key)
        .map_err(|e| DefiError::SecurityError(format!(ERR_FAILED_TO_INIT_CIPHER, e)))?;
    
    let mut nonce = [0u8; NONCE_SIZE];
    OsRng.fill_bytes(&mut nonce);
    
    let ciphertext = cipher
        .encrypt(&Nonce::from_slice(&nonce), data.as_bytes())
        .map_err(|e| DefiError::SecurityError(format!(ERR_ENCRYPTION_FAILED, e)))?;

    Ok((ciphertext, nonce.to_vec(), salt.to_vec()))
}

/// Decrypt data với custom key derivation function
pub fn decrypt_data_with_key(
    ciphertext: &[u8],
    nonce: &[u8],
    salt: &[u8],
    password: &str,
    key_salt: &str
) -> Result<String, DefiError> {
    // Tạo final key từ password, salt và key
    let derived_key = derive_encryption_key(password, salt, key_salt)?;
    
    let cipher = Aes256Gcm::new_from_slice(&derived_key)
        .map_err(|e| DefiError::SecurityError(format!(ERR_FAILED_TO_INIT_CIPHER, e)))?;
    
    let plaintext = cipher
        .decrypt(&Nonce::from_slice(nonce), ciphertext)
        .map_err(|e| DefiError::SecurityError(format!(ERR_DECRYPTION_FAILED, e)))?;

    String::from_utf8(plaintext)
        .map_err(|e| DefiError::SecurityError(format!(ERR_INVALID_UTF8, e)))
}

/// Tạo encryption key từ password, salt và master key
fn derive_encryption_key(
    password: &str, 
    salt: &[u8], 
    key_salt: &str
) -> Result<[u8; KEY_SIZE], DefiError> {
    // Step 1: Tạo key từ password và salt
    let mut password_key = [0u8; KEY_SIZE];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, PBKDF2_ITERATIONS, &mut password_key);
    
    // Step 2: Kết hợp với master key
    let mut combined_key = [0u8; KEY_SIZE];
    pbkdf2_hmac::<Sha256>(&password_key, key_salt.as_bytes(), 1000, &mut combined_key);
    
    Ok(combined_key)
}

/// (Ciphertext, nonce, salt) mã hóa với tham số tùy chỉnh.
///
/// # Examples
/// ```
/// use wallet::defi::crypto::encrypt_data_with_params;
///
/// let data = "sensitive_data";
/// let password = "secure_password";
/// let iterations = 200_000;
/// let salt_size = 32;
/// let (ciphertext, nonce, salt) = encrypt_data_with_params(data, password, iterations, salt_size).unwrap();
/// ```
pub fn encrypt_data_with_params(
    data: &str, 
    password: &str, 
    iterations: u32, 
    salt_size: usize
) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>), DefiError> {
    let mut salt = vec![0u8; salt_size];
    OsRng.fill_bytes(&mut salt);
    
    let mut key = [0u8; KEY_SIZE];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, iterations, &mut key);

    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| DefiError::SecurityError(format!(ERR_FAILED_TO_INIT_CIPHER, e)))?;
    
    let mut nonce = [0u8; NONCE_SIZE];
    OsRng.fill_bytes(&mut nonce);
    
    let ciphertext = cipher
        .encrypt(&Nonce::from_slice(&nonce), data.as_bytes())
        .map_err(|e| DefiError::SecurityError(format!(ERR_ENCRYPTION_FAILED, e)))?;

    Ok((ciphertext, nonce.to_vec(), salt))
}

/// (Ciphertext, nonce, salt) mã hóa.
///
/// # Examples
/// ```
/// use wallet::defi::crypto::encrypt_data;
///
/// let data = "sensitive_data";
/// let password = "secure_password";
/// let (ciphertext, nonce, salt) = encrypt_data(data, password).unwrap();
/// ```
pub fn encrypt_data(data: &str, password: &str) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>), DefiError> {
    let mut salt = [0u8; SALT_SIZE];
    OsRng.fill_bytes(&mut salt);
    
    let mut key = [0u8; KEY_SIZE];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, PBKDF2_ITERATIONS, &mut key);

    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| DefiError::SecurityError(format!(ERR_FAILED_TO_INIT_CIPHER, e)))?;
    
    let mut nonce = [0u8; NONCE_SIZE];
    OsRng.fill_bytes(&mut nonce);
    
    let ciphertext = cipher
        .encrypt(&Nonce::from_slice(&nonce), data.as_bytes())
        .map_err(|e| DefiError::SecurityError(format!(ERR_ENCRYPTION_FAILED, e)))?;

    Ok((ciphertext, nonce.to_vec(), salt.to_vec()))
}

/// Giải mã dữ liệu bằng mật khẩu.
///
/// # Arguments
/// - `ciphertext`: Dữ liệu đã mã hóa.
/// - `nonce`: Nonce dùng khi mã hóa.
/// - `salt`: Salt dùng khi mã hóa.
/// - `password`: Mật khẩu người dùng.
///
/// # Returns
/// Dữ liệu gốc.
///
/// # Examples
/// ```
/// use wallet::defi::crypto::{encrypt_data, decrypt_data};
///
/// let data = "sensitive_data";
/// let password = "secure_password";
/// let (ciphertext, nonce, salt) = encrypt_data(data, password).unwrap();
/// 
/// let decrypted = decrypt_data(&ciphertext, &nonce, &salt, password).unwrap();
/// assert_eq!(data, decrypted);
/// ```
pub fn decrypt_data(
    ciphertext: &[u8],
    nonce: &[u8],
    salt: &[u8],
    password: &str,
) -> Result<String, DefiError> {
    let mut key = [0u8; KEY_SIZE];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, PBKDF2_ITERATIONS, &mut key);

    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| DefiError::SecurityError(format!(ERR_FAILED_TO_INIT_CIPHER, e)))?;
    
    let plaintext = cipher
        .decrypt(&Nonce::from_slice(nonce), ciphertext)
        .map_err(|e| DefiError::SecurityError(format!(ERR_DECRYPTION_FAILED, e)))?;

    String::from_utf8(plaintext)
        .map_err(|e| DefiError::SecurityError(format!(ERR_INVALID_UTF8, e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt() {
        let data = "test token data";
        let password = "secure_password";
        
        let (ciphertext, nonce, salt) = encrypt_data(data, password)
            .expect("Encryption should succeed");
            
        let decrypted = decrypt_data(&ciphertext, &nonce, &salt, password)
            .expect("Decryption should succeed with correct password");
            
        assert_eq!(decrypted, data, "Decrypted data should match original");

        let result = decrypt_data(&ciphertext, &nonce, &salt, "wrong_password");
        assert!(result.is_err(), "Decryption should fail with wrong password");
    }

    #[test]
    fn test_wrong_salt() {
        let data = "test token data";
        let password = "secure_password";
        
        let (ciphertext, nonce, _) = encrypt_data(data, password)
            .expect("Encryption should succeed");
            
        let mut wrong_salt = [0u8; SALT_SIZE];
        OsRng.fill_bytes(&mut wrong_salt);
        
        let result = decrypt_data(&ciphertext, &nonce, &wrong_salt, password);
        assert!(result.is_err(), "Decryption should fail with wrong salt");
    }
    
    #[test]
    fn test_key_rotation() {
        let data = "test sensitive data";
        let password = "user_password";
        let master_key = "master_key_v1";
        
        // Mã hóa với key version 1
        let (ciphertext, nonce, salt, metadata) = encrypt_data_with_rotation(
            data, password, master_key
        ).expect("Encryption with rotation should succeed");
        
        // Giải mã với key version 1
        let decrypted = decrypt_data_with_rotation(
            &ciphertext, &nonce, &salt, &metadata, password, master_key
        ).expect("Decryption should succeed with correct key version");
        
        assert_eq!(decrypted, data, "Decrypted data should match original");
        
        // Tạo manager và rotate key
        let mut manager = KeyRotationManager::new(master_key, Some(90));
        let master_key_v2 = "master_key_v2";
        manager.rotate_key(master_key_v2);
        
        // Re-encrypt với key mới
        let (new_ciphertext, new_nonce, new_salt, new_metadata) = manager.reencrypt(
            &ciphertext, &nonce, &salt, &metadata, password
        ).expect("Re-encryption should succeed");
        
        // Giải mã với key mới
        let decrypted_after_rotation = decrypt_data_with_rotation(
            &new_ciphertext, &new_nonce, &new_salt, &new_metadata, 
            password, master_key_v2
        ).expect("Decryption after rotation should succeed");
        
        assert_eq!(decrypted_after_rotation, data, "Data after rotation should be preserved");
    }
    
    #[test]
    fn test_key_metadata() {
        let metadata = KeyMetadata::new(5, Some(90));
        let bytes = metadata.to_bytes();
        let recovered = KeyMetadata::from_bytes(&bytes).expect("Should decode metadata");
        
        assert_eq!(recovered.version, 5, "Version should match");
        assert_eq!(recovered.created_at, metadata.created_at, "Created timestamp should match");
        assert_eq!(recovered.expires_at, metadata.expires_at, "Expiry should match");
    }
} 