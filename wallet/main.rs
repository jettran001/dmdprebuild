// wallet/main.rs

use wallet::walletmanager::api::WalletManagerApi;
use wallet::config::WalletSystemConfig;
use wallet::walletmanager::types::{WalletConfig, SeedLength};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let config = WalletSystemConfig::default();
    let mut manager = WalletManagerApi::new(config);

    // Ví dụ: Tạo ví mới
    let result = manager.create_wallet(SeedLength::Twelve, None);
    if let Ok((address, seed_phrase, user_id)) = result {
        println!("Created wallet: {}, user_id: {}, seed: {}", address, user_id, seed_phrase);
    }

    // Ví dụ: Nhập ví từ private key
    let config = WalletConfig {
        seed_or_key: "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
        chain_id: 0, // Dùng default_chain_id
        seed_length: None,
    };
    let result = manager.import_wallet(config);
    if let Ok((address, user_id)) = result {
        println!("Imported wallet: {}, user_id: {}", address, user_id);
    }
}