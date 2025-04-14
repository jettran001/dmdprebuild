//! 🧭 Entry Point: Đây là manifest chính chứa toàn bộ module của dự án snipebot.
//! Mỗi thư mục là một đơn vị rõ ràng: `chain_adapters`, `tradelogic`, `registry`, `utils`.
//! Bot hãy bắt đầu từ đây để resolve module path chính xác.
//! Được dùng làm tài liệu tham chiếu khi import từ domain khác (ví dụ: wallet).

/*
    snipebot/
    ├── Cargo.toml                  -> Cấu hình dependencies
    ├── manifest.rs                 -> Tài liệu tham chiếu module path [liên quan: tất cả các module, BẮT BUỘC đọc đầu tiên]
    ├── src/main.rs                 -> Điểm chạy chính, bắt đầu snipebot [liên quan: lib.rs, config.rs]
    ├── src/lib.rs                  -> Khai báo module cấp cao, re-export [liên quan: tất cả module khác, điểm import cho crate]
    ├── src/config.rs               -> Cấu hình snipebot (chains, wallets, API keys) [liên quan: lib.rs, chain_adapters]
    ├── src/types.rs                -> Kiểu dữ liệu dùng chung cho snipebot [liên quan: tất cả module khác]
    ├── src/metric.rs               -> Thu thập và báo cáo metrics [liên quan: cache.rs, health.rs]
    ├── src/cache.rs                -> Bộ nhớ cache cho dữ liệu blockchain [liên quan: chain_adapters, metric.rs]
    ├── src/health.rs               -> Kiểm tra tình trạng hệ thống [liên quan: metric.rs]
    ├── src/utils.rs                -> Các hàm tiện ích dùng chung [liên quan: tất cả file khác]
    ├── src/analys.rs               -> Phân tích dữ liệu market [liên quan: tradelogic, chain_adapters]
    ├── src/chain_adapters/         -> Bộ adapter cho các blockchain khác nhau
    │   ├── evm_adapter.rs          -> Adapter cho EVM chains (Ethereum, BSC, Polygon) [liên quan: config.rs, cache.rs]
    │   ├── sol_adapter.rs          -> Adapter cho Solana [liên quan: config.rs, cache.rs]
    │   ├── stellar_adapter.rs      -> Adapter cho Stellar [liên quan: config.rs, cache.rs]
    │   ├── sui_adapter.rs          -> Adapter cho Sui [liên quan: config.rs, cache.rs]
    ├── src/tradelogic/             -> Logic giao dịch
    │   ├── manual_trade.rs         -> Giao dịch thủ công do người dùng khởi tạo [liên quan: chain_adapters, metric.rs]
    │   ├── mev_logic.rs            -> Logic phát hiện và tận dụng MEV [liên quan: chain_adapters, analys.rs]
    ├── src/registry/              -> Registry cho các module
    │   ├── manifest.rs            -> Bản đồ module tương tự như file này [liên quan: tất cả các module]

    Mối liên kết:
    - chain_adapters là trung tâm tương tác với các blockchain
    - evm_adapter.rs kết nối với Ethereum, BSC, Polygon và các EVM chains khác
    - sol_adapter.rs kết nối với Solana blockchain
    - stellar_adapter.rs kết nối với Stellar blockchain
    - sui_adapter.rs kết nối với Sui blockchain
    - tradelogic chứa các chiến lược giao dịch
    - manual_trade.rs xử lý các giao dịch người dùng khởi tạo
    - mev_logic.rs tìm và tận dụng cơ hội MEV (Miner Extractable Value)
    - config.rs quản lý cấu hình cho các chain và API keys
    - cache.rs lưu trữ tạm thời dữ liệu blockchain để tối ưu performance
    - health.rs giám sát tình trạng hệ thống
    - metric.rs thu thập metrics về giao dịch và hiệu suất
    - utils.rs cung cấp các hàm tiện ích dùng chung
    - analys.rs phân tích dữ liệu thị trường và xu hướng
    - registry/manifest.rs là bản đồ module tương tự như file này
    - snipebot tương tác với domain wallet qua wallet::walletmanager::api
    - snipebot sử dụng ví từ wallet để thực hiện giao dịch
    - tradelogic kiểm tra quyền hạn người dùng qua wallet::users
*/

// Module structure của dự án snipebot
pub mod chain_adapters; // Adapter cho các blockchain khác nhau
pub mod tradelogic;    // Logic giao dịch
pub mod registry;      // Registry cho các module
pub mod config;        // Cấu hình
pub mod types;         // Kiểu dữ liệu
pub mod cache;         // Cache dữ liệu
pub mod metric;        // Metrics
pub mod health;        // Health check
pub mod utils;         // Utility functions
pub mod analys;        // Phân tích dữ liệu

/**
 * Hướng dẫn import:
 * 
 * 1. Import từ internal crates:
 * - use crate::chain_adapters::evm_adapter::EvmAdapter;
 * - use crate::chain_adapters::sol_adapter::SolanaAdapter;
 * - use crate::tradelogic::manual_trade::ManualTradeExecutor;
 * - use crate::tradelogic::mev_logic::MevStrategy;
 * - use crate::config::SnipebotConfig;
 * - use crate::types::{TradeParams, ChainType, TokenPair};
 * - use crate::cache::BlockchainCache;
 * - use crate::metric::TradeMetrics;
 * - use crate::utils::helpers;
 * 
 * 2. Import từ external crates (từ wallet):
 * - use wallet::walletmanager::api::WalletManagerApi;
 * - use wallet::users::subscription::manager::SubscriptionManager;
 * - use wallet::users::vip_user::VipUserManager;
 * - use wallet::users::premium_user::PremiumUserManager;
 * 
 * 3. Import từ third-party libraries:
 * - use ethers::providers::{Provider, Http};
 * - use ethers::types::{Address, U256, Transaction};
 * - use solana_client::rpc_client::RpcClient;
 * - use stellar_sdk::Client as StellarClient;
 * - use sui_sdk::client::SuiClient;
 * - use tokio::sync::{RwLock, Mutex};
 */
