//! 🧭 Entry Point: Manifest for snipebot domain - DiamondChain SnipeBot DeFi
//!
//! Cấu trúc module thực tế, mô tả chức năng, routes, trait, mối liên hệ giữa các module.
//!
//! # Tổng quan
//! - Core logic: tradelogic (manual, smart, MEV)
//! - Phân tích: analys (token, mempool, risk)
//! - Adapter: chain_adapters (blockchain interaction)
//! - Hỗ trợ: cache, metric, health, config, types
//!
//! # Cây module thực tế
//!
//! snipebot/
//! ├── src/
//! │   ├── main.rs                // Entry point (main binary)
//! │   ├── lib.rs                 // Library entry, re-export modules
//! │   ├── config.rs              // Bot configuration (chains, keys, strategies)
//! │   ├── types.rs               // Common types for snipebot
//! │   ├── cache.rs               // Blockchain data cache
//! │   ├── metric.rs              // Metrics collection/reporting
//! │   ├── health.rs              // Health check
//! │   ├── chain_adapters/        // Blockchain adapters
//! │   │   ├── mod.rs
//! │   │   ├── evm_adapter.rs     // EVM chain adapter (Ethereum, BSC, ...)
//! │   │   ├── sol_adapter.rs     // Solana adapter (stub)
//! │   │   ├── stellar_adapter.rs // Stellar adapter (stub)
//! │   │   ├── sui_adapter.rs     // Sui adapter (stub)
//! │   │   ├── ton_adapter.rs     // TON adapter (stub)
//! │   ├── tradelogic/            // Trading logic (core)
//! │   │   ├── mod.rs
//! │   │   ├── traits.rs          // Common traits for all strategies
//! │   │   ├── manual_trade.rs    // User-initiated trading (API)
//! │   │   ├── smart_trade/       // Automated smart trading
//! │   │   │   ├── mod.rs
//! │   │   │   ├── executor.rs    // SmartTradeExecutor (trait impl)
//! │   │   │   ├── types.rs, constants.rs, utils.rs, ...
//! │   │   │   ├── trade_strategy.rs, token_analysis.rs, ...
//! │   │   │   ├── analys_client.rs // Client để sử dụng analys API
//! │   │   ├── mev_logic/         // MEV logic (modular, advanced)
//! │   │   │   ├── mod.rs
//! │   │   │   ├── bot.rs         // MevBot (trait impl)
//! │   │   │   ├── opportunity.rs // MevOpportunity struct
//! │   │   │   ├── types.rs, strategy.rs, analyzer.rs, ...
//! │   │   │   ├── jit_liquidity.rs, cross_domain.rs, ...
//! │   │   ├── common/            // Shared types/utils for tradelogic
//! │   │   │   ├── types.rs, utils.rs, analysis.rs
//! │   ├── analys/                // Data analysis (token, mempool, risk)
//! │   │   ├── mod.rs
//! │   │   ├── api/               // API for tradelogic to use analysis services
//! │   │   │   ├── mod.rs         // API module coordination and factory functions 
//! │   │   │   ├── mempool_api.rs // Mempool analysis provider implementation
//! │   │   │   ├── token_api.rs   // Token analysis provider implementation
//! │   │   │   ├── risk_api.rs    // Risk analysis provider implementation
//! │   │   │   ├── mev_opportunity_api.rs // MEV opportunity provider implementation
//! │   │   ├── mempool/           // Mempool analysis
//! │   │   │   ├── mod.rs, types.rs, arbitrage.rs, ...
//! │   │   ├── token_status/      // Token safety analysis
//! │   │   │   ├── mod.rs, types.rs, utils.rs, ...
//! │   │   ├── risk_analyzer.rs   // Risk analysis/aggregation
//!
//! # Mô tả chức năng từng module
//!
//! - **main.rs**: Điểm khởi động bot, load config, spawn các executor.
//! - **lib.rs**: Re-export các module chính, entry cho crate.
//! - **config.rs**: Định nghĩa cấu hình bot (API keys, chain, chiến lược, risk...)
//! - **types.rs**: Kiểu dữ liệu chung (TradeParams, ChainType, TokenPair...)
//! - **cache.rs**: Bộ nhớ tạm, cache dữ liệu blockchain để tối ưu hiệu suất.
//! - **metric.rs**: Thu thập, báo cáo metrics (giao dịch, hiệu suất, cảnh báo).
//! - **health.rs**: Kiểm tra tình trạng hệ thống, endpoint health check.
//!
//! ## chain_adapters
//! - **evm_adapter.rs**: Adapter chuẩn cho các EVM chain (Ethereum, BSC, Polygon...).
//! - **sol_adapter.rs, stellar_adapter.rs, sui_adapter.rs, ton_adapter.rs**: Adapter cho các chain khác (chưa triển khai).
//!
//! ## tradelogic
//! - **traits.rs**: Định nghĩa trait chuẩn cho mọi chiến lược (TradeExecutor, RiskManager, ...) và các trait cho MEV (MempoolAnalysisProvider, TokenAnalysisProvider, RiskAnalysisProvider, MevOpportunityProvider).
//! - **manual_trade.rs**: Giao dịch thủ công do user khởi tạo qua API, kiểm soát risk, validate token, lưu lịch sử.
//! - **smart_trade/**: Giao dịch tự động thông minh, quản lý chiến lược, risk, TSL, auto-sell, adaptive.
//!   - **executor.rs**: SmartTradeExecutor, implement trait TradeExecutor, quản lý toàn bộ flow giao dịch tự động.
//!   - **trade_strategy.rs, token_analysis.rs, ...**: Các chiến lược, phân tích token, tối ưu hóa, cảnh báo.
//!   - **analys_client.rs**: Client để sử dụng các dịch vụ phân tích từ analys/api, bao gồm phân tích token, mempool, risk và cơ hội MEV.
//! - **mev_logic/**: Logic MEV chuyên sâu (arbitrage, sandwich, JIT, cross-domain, liquidation...)
//!   - **bot.rs**: MevBot, implement trait TradeExecutor, phát hiện và tận dụng cơ hội MEV.
//!   - **opportunity.rs**: Định nghĩa MevOpportunity, risk, execution, trạng thái, và OpportunityManager để quản lý cơ hội.
//!   - **jit_liquidity.rs, cross_domain.rs, ...**: JIT liquidity, cross-chain MEV, bundle, analyzer, strategy...
//! - **common/**: Kiểu dữ liệu, util, phân tích dùng chung cho tradelogic.
//!
//! ## analys
//! - **api/**: Interface để tradelogic/mev_logic sử dụng chức năng phân tích.
//!   - **mod.rs**: Tổng hợp và cung cấp các factory function để tạo các provider.
//!   - **mempool_api.rs**: Triển khai MempoolAnalysisProvider để phân tích giao dịch mempool.
//!   - **token_api.rs**: Triển khai TokenAnalysisProvider cho phân tích an toàn token.
//!   - **risk_api.rs**: Triển khai RiskAnalysisProvider cho đánh giá rủi ro.
//!   - **mev_opportunity_api.rs**: Triển khai MevOpportunityProvider để quản lý cơ hội MEV.
//! - **mempool/**: Phân tích mempool, phát hiện arbitrage, filter, detection, analyzer.
//! - **token_status/**: Phân tích an toàn token, blacklist, tax, owner, liquidity, utils.
//! - **risk_analyzer.rs**: Tổng hợp risk từ token, mempool, lịch sử, đưa ra khuyến nghị.
//!
//! # Routes & Traits
//! - **TradeExecutor**: Trait chuẩn cho mọi chiến lược giao dịch (manual, smart, MEV), định nghĩa interface start/stop, execute, status, history, risk.
//! - **RiskManager, StrategyOptimizer, CrossChainTrader**: Trait mở rộng cho risk, tối ưu hóa, cross-chain.
//! - **MevBot, SmartTradeExecutor, ManualTradeExecutor**: Các struct implement trait chuẩn, có thể hoán đổi, mở rộng.
//! - **MempoolAnalysisProvider, TokenAnalysisProvider, RiskAnalysisProvider, MevOpportunityProvider**: Các trait chuẩn cho phân tích và cung cấp input cho mev_logic.
//! - **Flow**: analys (mempool/token/risk) <-> tradelogic (executor) <-> chain_adapters (blockchain)
//! - **API**: manual_trade expose API cho user, smart_trade/mev_logic chạy tự động.
//!
//! # Mối liên hệ
//! - **tradelogic** là core, gọi analys để phân tích, gọi chain_adapters để thực thi giao dịch.
//! - **analys** cung cấp phân tích token, mempool, risk cho tradelogic thông qua các API provider.
//! - **chain_adapters** là gateway tới blockchain thực tế.
//! - **common/types** chia sẻ kiểu dữ liệu, util giữa các module.
//! - **mev_logic** sử dụng APIs từ analys/api để phát hiện và thực thi cơ hội MEV.
//! - **smart_trade** sử dụng SmartTradeAnalysisClient để tương tác với các API của analys/api.
//!
//! # Cập nhật manifest này khi có thay đổi module thực tế.
