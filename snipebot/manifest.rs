//! 🧭 Entry Point: Đây là manifest chính chứa toàn bộ module của dự án snipebot.
//! Mỗi thư mục là một đơn vị rõ ràng: `chain_adapters`, `tradelogic`, `analys.rs`, `utils.rs`.
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
    ├── src/chain_adapters/         -> Bộ adapter cho các blockchain khác nhau
    │   ├── evm_adapter.rs          -> Adapter cho EVM chains (Ethereum, BSC, Polygon) [liên quan: config.rs, cache.rs]
    │   ├── sol_adapter.rs          -> Adapter cho Solana [liên quan: config.rs, cache.rs]
    │   ├── stellar_adapter.rs      -> Adapter cho Stellar [liên quan: config.rs, cache.rs]
    │   ├── sui_adapter.rs          -> Adapter cho Sui [liên quan: config.rs, cache.rs]
    │   ├── ton_adapter.rs          -> Adapter cho TON blockchain [liên quan: config.rs, cache.rs]
    ├── src/tradelogic/             -> Logic giao dịch
    │   ├── manual_trade.rs         -> Giao dịch thủ công do người dùng khởi tạo [liên quan: chain_adapters, metric.rs]
    │   ├── mev_logic.rs            -> Logic phát hiện và tận dụng MEV [liên quan: chain_adapters, analys.rs]
    │   ├── smart_trade.rs          -> Giao dịch tự động với các chiến lược thông minh [liên quan: chain_adapters, analys.rs]
    ├── src/analys.rs/              -> Phân tích dữ liệu market
    │   ├── token_status.rs         -> Kiểm tra và phân tích trạng thái token [liên quan: chain_adapters, tradelogic]
    │   ├── mempool.rs              -> Phân tích giao dịch trong mempool [liên quan: chain_adapters, tradelogic]
    │   ├── risk_analyzer.rs        -> Phân tích rủi ro giao dịch [liên quan: chain_adapters, tradelogic]

    Mối liên kết:
    - chain_adapters là trung tâm tương tác với các blockchain
    - evm_adapter.rs kết nối với Ethereum, BSC, Polygon và các EVM chains khác
    - sol_adapter.rs kết nối với Solana blockchain
    - stellar_adapter.rs kết nối với Stellar blockchain
    - sui_adapter.rs kết nối với Sui blockchain
    - ton_adapter.rs kết nối với TON blockchain
    - tradelogic chứa các chiến lược giao dịch
    - manual_trade.rs xử lý các giao dịch người dùng khởi tạo
    - mev_logic.rs tìm và tận dụng cơ hội MEV (Miner Extractable Value)
    - smart_trade.rs thực hiện giao dịch với các chiến lược tự động
    - config.rs quản lý cấu hình cho các chain và API keys
    - cache.rs lưu trữ tạm thời dữ liệu blockchain để tối ưu performance
    - health.rs giám sát tình trạng hệ thống
    - metric.rs thu thập metrics về giao dịch và hiệu suất
    - types.rs định nghĩa các kiểu dữ liệu dùng chung
    - analys.rs/token_status.rs phân tích trạng thái và độ an toàn của token
    - analys.rs/mempool.rs phân tích các giao dịch đang chờ trong mempool
    - analys.rs/risk_analyzer.rs đánh giá rủi ro của giao dịch
    - snipebot tương tác với domain wallet qua wallet::walletmanager::api
    - snipebot sử dụng ví từ wallet để thực hiện giao dịch
    - tradelogic kiểm tra quyền hạn người dùng qua wallet::users
*/

// Module structure của dự án snipebot
pub mod chain_adapters; // Adapter cho các blockchain khác nhau
pub mod tradelogic;    // Logic giao dịch
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
 * - use crate::chain_adapters::stellar_adapter::StellarAdapter;
 * - use crate::chain_adapters::sui_adapter::SuiAdapter;
 * - use crate::chain_adapters::ton_adapter::TonAdapter;
 * - use crate::tradelogic::manual_trade::ManualTradeExecutor;
 * - use crate::tradelogic::mev_logic::MevStrategy;
 * - use crate::tradelogic::smart_trade::SmartTradeExecutor;
 * - use crate::analys::token_status::TokenStatusAnalyzer;
 * - use crate::analys::mempool::MempoolAnalyzer;
 * - use crate::analys::risk_analyzer::RiskAnalyzer;
 * - use crate::config::SnipebotConfig;
 * - use crate::types::{TradeParams, ChainType, TokenPair};
 * - use crate::cache::BlockchainCache;
 * - use crate::metric::TradeMetrics;
 * - use crate::health::HealthChecker;
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
 * - use ton_client::TonClient;
 * - use tokio::sync::{RwLock, Mutex};
 * - use serde::{Serialize, Deserialize};
 * - use tracing::{info, error, debug, warn};
 */

// SnipeBot Module - DiamondChain
// Cung cấp công cụ giao dịch và snipe token trên nhiều blockchain khác nhau

/// Các trait chính trong snipebot domain:
/**
 * ChainAdapter (từ các file adapter):
 * - Trait cho tương tác với các blockchain khác nhau
 * - Được implement bởi: EvmAdapter, SolanaAdapter, StellarAdapter, SuiAdapter, TonAdapter
 * - Phương thức chính: get_balance, send_transaction, get_token_price, approve_token, swap_tokens
 * 
 * TradeExecutor (từ tradelogic):
 * - Trait cho các chiến lược giao dịch
 * - Được implement bởi: ManualTradeExecutor, MevStrategy, SmartTradeExecutor
 * - Phương thức chính: execute_trade, calculate_profit, estimate_gas, validate_trade
 * 
 * Analyzer (từ analys.rs):
 * - Trait cho phân tích thông tin token và giao dịch
 * - Được implement bởi: TokenStatusAnalyzer, MempoolAnalyzer, RiskAnalyzer
 * - Phương thức chính: analyze_token, check_safety, calculate_risk, monitor_mempool
 */

// Các cập nhật quan trọng:
/**
 * 05-06-2023: Khởi tạo cấu trúc module snipebot
 * 08-06-2023: Thêm module chain_adapters với evm_adapter.rs
 * 10-06-2023: Thêm module tradelogic với manual_trade.rs
 * 12-06-2023: Thêm metric.rs và cache.rs
 * 15-06-2023: Thêm sol_adapter.rs cho Solana blockchain
 * 18-06-2023: Thêm tradelogic/mev_logic.rs
 * 20-06-2023: Thêm stellar_adapter.rs và sui_adapter.rs
 * 22-06-2023: Thêm health.rs để giám sát hệ thống
 * 25-06-2023: Thêm analys.rs/token_status.rs
 * 28-06-2023: Thêm ton_adapter.rs cho TON blockchain
 * 01-07-2023: Thêm smart_trade.rs vào tradelogic
 * 03-07-2023: Thêm analys.rs/mempool.rs
 * 05-07-2023: Thêm analys.rs/risk_analyzer.rs
 * 08-07-2023: Cập nhật manifest.rs để phản ánh cấu trúc thực tế của dự án
 */
