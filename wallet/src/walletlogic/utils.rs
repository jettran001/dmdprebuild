// wallet/src/walletlogic/utils.rs

use uuid::Uuid;

use crate::walletmanager::types::WalletConfig;

/// Tạo ID người dùng duy nhất.
///
/// # Returns
/// Trả về UUID dạng chuỗi.
pub fn generate_user_id() -> String {
    Uuid::new_v4().to_string()
}

/// Kiểm tra xem config có chứa seed phrase không.
///
/// # Arguments
/// - `config`: Cấu hình ví.
///
/// # Returns
/// `true` nếu là seed phrase (12 hoặc 24 từ), `false` nếu là private key.
pub fn is_seed_phrase(config: &WalletConfig) -> bool {
    matches!(
        config.seed_length,
        Some(crate::walletmanager::types::SeedLength::Twelve)
            | Some(crate::walletmanager::types::SeedLength::TwentyFour)
    )
}