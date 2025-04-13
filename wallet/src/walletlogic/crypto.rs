// External imports
use aes_gcm::{aead::{Aead, KeyInit}, Aes256Gcm, Nonce};
use pbkdf2::pbkdf2_hmac;
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::Sha256;

// Internal imports
use crate::error::WalletError;

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

/// Mã hóa dữ liệu bằng mật khẩu.
///
/// # Arguments
/// - `data`: Dữ liệu cần mã hóa (seed/private key).
/// - `password`: Mật khẩu người dùng.
///
/// # Returns
/// (Ciphertext, nonce, salt) mã hóa.
///
/// # Flow
/// Dữ liệu từ `walletlogic::init` để mã hóa khóa.
#[flow_from("walletlogic::init")]
pub fn encrypt_data(data: &str, password: &str) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>), WalletError> {
    let mut salt = [0u8; SALT_SIZE];
    OsRng.fill_bytes(&mut salt);
    
    let mut key = [0u8; KEY_SIZE];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, PBKDF2_ITERATIONS, &mut key);

    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| WalletError::EncryptionError(format!(ERR_FAILED_TO_INIT_CIPHER, e)))?;
    
    let mut nonce = [0u8; NONCE_SIZE];
    OsRng.fill_bytes(&mut nonce);
    
    let ciphertext = cipher
        .encrypt(&Nonce::from_slice(&nonce), data.as_bytes())
        .map_err(|e| WalletError::EncryptionError(format!(ERR_ENCRYPTION_FAILED, e)))?;

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
/// Dữ liệu gốc (seed/private key).
///
/// # Flow
/// Dữ liệu từ `walletlogic::handler` để giải mã khóa.
#[flow_from("walletlogic::handler")]
pub fn decrypt_data(
    ciphertext: &[u8],
    nonce: &[u8],
    salt: &[u8],
    password: &str,
) -> Result<String, WalletError> {
    let mut key = [0u8; KEY_SIZE];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, PBKDF2_ITERATIONS, &mut key);

    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| WalletError::DecryptionError(format!(ERR_FAILED_TO_INIT_CIPHER, e)))?;
    
    let plaintext = cipher
        .decrypt(&Nonce::from_slice(nonce), ciphertext)
        .map_err(|e| WalletError::DecryptionError(format!(ERR_DECRYPTION_FAILED, e)))?;

    String::from_utf8(plaintext)
        .map_err(|e| WalletError::DecryptionError(format!(ERR_INVALID_UTF8, e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt() {
        let data = "test seed phrase";
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
        let data = "test seed phrase";
        let password = "secure_password";
        
        let (ciphertext, nonce, _) = encrypt_data(data, password)
            .expect("Encryption should succeed");
            
        let mut wrong_salt = [0u8; SALT_SIZE];
        OsRng.fill_bytes(&mut wrong_salt);
        
        let result = decrypt_data(&ciphertext, &nonce, &wrong_salt, password);
        assert!(result.is_err(), "Decryption should fail with wrong salt");
    }
}