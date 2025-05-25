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
//! │   ├── lib.rs                 // Library entry, re-export modules, global coordinator
//! │   ├── config.rs              // Bot configuration (chains, keys, strategies)
//! │   ├── types.rs               // Common types for snipebot
//! │   ├── cache.rs               // Blockchain data cache
//! │   ├── metric.rs              // Metrics collection/reporting
//! │   ├── health.rs              // Health check
//! │   ├── chain_adapters/        // Blockchain adapters
//! │   │   ├── mod.rs             // Module exports và bridge_types re-export
//! │   │   ├── evm_adapter.rs     // EVM chain adapter (Ethereum, BSC, ...)
//! │   │   ├── sol_adapter.rs     // Solana adapter (stub)
//! │   │   ├── stellar_adapter.rs // Stellar adapter (stub)
//! │   │   ├── sui_adapter.rs     // Sui adapter (stub)
//! │   │   ├── ton_adapter.rs     // TON adapter (stub)
//! │   │   ├── bridge_adapter.rs  // Cross-chain bridge adapter
//! │   ├── tradelogic/            // Trading logic (core)
//! │   │   ├── mod.rs             // Module exports
//! │   │   ├── traits.rs          // Common traits cho all trading strategies
//! │   │   ├── coordinator.rs     // TradeCoordinator implementation
//! │   │   ├── manual_trade.rs    // Manual trading implementation
//! │   │   ├── smart_trade/       // Automated smart trading
//! │   │   │   ├── mod.rs         // Module exports
//! │   │   │   ├── executor.rs    // SmartTradeExecutor (TradeExecutor implementation)
//! │   │   │   ├── analys_client.rs // Client sử dụng analys API
//! │   │   │   ├── security.rs    // Security checks và phân tích
//! │   │   │   ├── anti_mev.rs    // Chống MEV attacks
//! │   │   │   ├── optimization.rs // Tối ưu hóa strategies
//! │   │   │   ├── optimizer.rs   // Gas optimizer
//! │   │   │   ├── alert.rs       // Hệ thống cảnh báo 
//! │   │   │   ├── types.rs       // Kiểu dữ liệu riêng cho smart trade
//! │   │   │   ├── constants.rs   // Các hằng số
//! │   │   │   ├── token_analysis.rs // Phân tích token
//! │   │   │   ├── trade_strategy.rs // Chiến lược giao dịch
//! │   │   │   ├── utils.rs       // Utilities riêng cho smart trade
//! │   │   ├── mev_logic/         // MEV logic (modular, advanced)
//! │   │   │   ├── mod.rs         // Module coordination & re-exports
//! │   │   │   ├── types.rs       // MEV types và configs
//! │   │   │   ├── bot.rs         // MevBot trait & implementation
//! │   │   │   ├── strategy.rs    // MEV strategies
//! │   │   │   ├── opportunity.rs // MevOpportunity & OpportunityManager
//! │   │   │   ├── execution.rs   // MEV execution logic
//! │   │   │   ├── analysis.rs    // Phân tích MEV
//! │   │   │   ├── analyzer.rs    // MEV analyzer nâng cao
//! │   │   │   ├── trader_behavior.rs // Phân tích hành vi trader
//! │   │   │   ├── bundle.rs      // Bundle transactions
//! │   │   │   ├── constants.rs   // MEV constants
//! │   │   │   ├── jit_liquidity.rs // Just-In-Time liquidity
//! │   │   │   ├── cross_domain.rs // Cross-domain MEV
//! │   │   │   ├── utils.rs       // MEV utilities
//! │   │   ├── common/            // Shared code for tradelogic
//! │   │   │   ├── mod.rs         // Module exports
//! │   │   │   ├── types.rs       // Common types
//! │   │   │   ├── utils.rs       // Shared utilities
//! │   │   │   ├── analysis.rs    // Shared analysis functions
//! │   │   │   ├── gas.rs         // Gas optimization utils
//! │   ├── analys/                // Data analysis (token, mempool, risk)
//! │   │   ├── mod.rs             // Module exports
//! │   │   ├── api/               // API for tradelogic to use analysis services
//! │   │   │   ├── mod.rs         // Module coordination
//! │   │   │   ├── factory.rs     // Factory functions để tạo providers
//! │   │   │   ├── mempool_api.rs // Mempool analysis provider implementation
//! │   │   │   ├── token_api.rs   // Token analysis provider implementation
//! │   │   │   ├── risk_api.rs    // Risk analysis provider implementation
//! │   │   │   ├── mev_opportunity_api.rs // MEV opportunity provider implementation
//! │   │   ├── mempool/           // Mempool analysis
//! │   │   │   ├── mod.rs         // Module exports
//! │   │   │   ├── types.rs       // Mempool datatypes
//! │   │   │   ├── analyzer.rs    // Mempool analysis core
//! │   │   │   ├── arbitrage.rs   // Arbitrage detection
//! │   │   │   ├── detection.rs   // Pattern detection
//! │   │   │   ├── filter.rs      // Transaction filtering
//! │   │   │   ├── priority.rs    // Priority scoring
//! │   │   │   ├── utils.rs       // Mempool utilities
//! │   │   ├── token_status/      // Token safety analysis
//! │   │   │   ├── mod.rs         // Module exports
//! │   │   │   ├── types.rs       // Token analysis types 
//! │   │   │   ├── blacklist.rs   // Blacklist detection
//! │   │   │   ├── liquidity.rs   // Liquidity analysis
//! │   │   │   ├── owner.rs       // Owner privilege analysis
//! │   │   │   ├── tax.rs         // Tax/fee detection
//! │   │   │   ├── utils.rs       // Token analysis utilities
//! │   │   ├── risk_analyzer.rs   // Risk analysis/aggregation
//!
//! # Mô tả chức năng từng module
//!
//! - **main.rs**: Điểm khởi động bot, khởi tạo AppState, load config, khởi chạy các service.
//! - **lib.rs**: Re-export các module chính, quản lý GLOBAL_COORDINATOR, cung cấp AppState (orchestrator).
//! - **config.rs**: Định nghĩa cấu hình bot (API keys, chain, chiến lược, risk), quản lý cấu hình động.
//! - **types.rs**: Kiểu dữ liệu chung (TradeParams, ChainType, TokenPair, TradeType).
//! - **cache.rs**: Bộ nhớ tạm, cache dữ liệu blockchain để tối ưu hiệu suất.
//! - **metric.rs**: Thu thập, báo cáo metrics (giao dịch, hiệu suất, cảnh báo).
//! - **health.rs**: Kiểm tra tình trạng hệ thống, endpoint health check, theo dõi RPC connections.
//!
//! ## chain_adapters
//! - **mod.rs**: Re-export bridge_types từ common module và các adapter khác.
//! - **evm_adapter.rs**: Adapter chuẩn cho các EVM chain (Ethereum, BSC, Polygon...).
//! - **bridge_adapter.rs**: Cung cấp chức năng cross-chain bridges, sử dụng bridge_types từ common.
//! - **sol_adapter.rs, stellar_adapter.rs, sui_adapter.rs, ton_adapter.rs**: Adapter cho các chain khác (stub).
//!
//! ## tradelogic
//! - **traits.rs**: Định nghĩa các trait chuẩn cho giao dịch:
//!   - **TradeExecutor**: Interface chung cho tất cả executor
//!   - **RiskManager**: Quản lý rủi ro
//!   - **StrategyOptimizer**: Tối ưu hóa chiến lược
//!   - **CrossChainTrader**: Giao dịch cross-chain
//!   - **MempoolAnalysisProvider, TokenAnalysisProvider, RiskAnalysisProvider, MevOpportunityProvider**: Các provider trait
//!   - **TradeCoordinator**: Điều phối giao dịch giữa các executor
//! - **coordinator.rs**: Triển khai TradeCoordinator để điều phối cơ hội giao dịch giữa các executor.
//! - **manual_trade.rs**: Giao dịch thủ công do user khởi tạo qua API, triển khai TradeExecutor.
//! - **smart_trade/**: Giao dịch tự động thông minh
//!   - **executor.rs**: Triển khai TradeExecutor, quản lý toàn bộ flow giao dịch tự động.
//!   - **analys_client.rs**: Client để sử dụng các dịch vụ phân tích từ analys/api.
//!   - **security.rs**: Kiểm tra bảo mật cho token và giao dịch.
//!   - **anti_mev.rs**: Phòng chống MEV attacks (front-running, sandwich).
//!   - **optimization.rs**: Tối ưu hóa các chiến lược giao dịch.
//!   - **optimizer.rs**: Tối ưu hóa gas và lựa chọn pool tối ưu.
//!   - **alert.rs**: Hệ thống cảnh báo thời gian thực (Telegram, Discord, Email).
//!   - **token_analysis.rs, trade_strategy.rs**: Phân tích token và chiến lược giao dịch.
//! - **mev_logic/**: Logic MEV chuyên sâu
//!   - **bot.rs**: Định nghĩa MevBot trait và triển khai MevBotImpl.
//!   - **opportunity.rs**: Định nghĩa MevOpportunity và OpportunityManager.
//!   - **jit_liquidity.rs**: JIT liquidity provider & opportunities.
//!   - **cross_domain.rs**: Cross-domain MEV & arbitrage.
//!   - **trader_behavior.rs**: Phân tích hành vi trader.
//!   - **bundle.rs**: Bundle transactions để thực thi MEV.
//!   - **analyzer.rs**: Các hàm phân tích MEV nâng cao.
//! - **common/**: Code dùng chung cho tradelogic
//!   - **types.rs**: Kiểu dữ liệu chung (RiskScore, SecurityCheckResult, TokenIssue).
//!   - **utils.rs**: Utilities chung cho tradelogic.
//!   - **analysis.rs**: Các hàm phân tích dùng chung.
//!   - **gas.rs**: Tối ưu hóa gas và utilities.
//!
//! ## analys
//! - **api/**: Interface cho tradelogic sử dụng
//!   - **mod.rs**: Module coordination
//!   - **factory.rs**: Factory functions để tạo các provider
//!   - **mempool_api.rs**: Triển khai MempoolAnalysisProvider
//!   - **token_api.rs**: Triển khai TokenAnalysisProvider
//!   - **risk_api.rs**: Triển khai RiskAnalysisProvider
//!   - **mev_opportunity_api.rs**: Triển khai MevOpportunityProvider
//! - **mempool/**: Phân tích mempool
//!   - **analyzer.rs**: Phân tích mempool core
//!   - **arbitrage.rs**: Phát hiện arbitrage
//!   - **detection.rs**: Phát hiện pattern
//!   - **filter.rs**: Lọc transaction
//!   - **priority.rs**: Điểm ưu tiên
//!   - **types.rs**: Kiểu dữ liệu mempool
//! - **token_status/**: Phân tích an toàn token
//!   - **blacklist.rs**: Phát hiện blacklist/anti-bot
//!   - **liquidity.rs**: Phân tích thanh khoản
//!   - **owner.rs**: Phân tích quyền hạn owner
//!   - **tax.rs**: Phát hiện thuế/phí
//!   - **types.rs**: Kiểu dữ liệu phân tích token
//! - **risk_analyzer.rs**: Tổng hợp risk từ token, mempool, lịch sử
//!
//! # Routes & Traits
//!
//! ## Core Traits
//! - **TradeExecutor**: Trait chuẩn cho mọi chiến lược giao dịch, định nghĩa interface start/stop, execute, status, history, risk.
//!   - Được triển khai bởi: `ManualTradeExecutor`, `SmartTradeExecutor`, `MevBotImpl`
//! - **TradeCoordinator**: Điều phối cơ hội giao dịch giữa các executor, quản lý chia sẻ cơ hội.
//!   - Được triển khai bởi: Coordinator trong `tradelogic/coordinator.rs`
//!   - Global instance: `lib.rs::GLOBAL_COORDINATOR`
//!
//! ## Analysis Provider Traits
//! - **MempoolAnalysisProvider**: Cung cấp phân tích mempool và phát hiện patterns.
//!   - Được triển khai bởi: `mempool_api.rs`
//! - **TokenAnalysisProvider**: Cung cấp phân tích token và bảo mật.
//!   - Được triển khai bởi: `token_api.rs` 
//! - **RiskAnalysisProvider**: Cung cấp đánh giá rủi ro.
//!   - Được triển khai bởi: `risk_api.rs`
//! - **MevOpportunityProvider**: Cung cấp và quản lý cơ hội MEV.
//!   - Được triển khai bởi: `mev_opportunity_api.rs`
//!
//! ## Specialized Traits
//! - **RiskManager**: Quản lý rủi ro trong giao dịch.
//! - **StrategyOptimizer**: Tối ưu hóa chiến lược giao dịch.
//! - **CrossChainTrader**: Giao dịch cross-chain.
//! - **MevBot**: Bot MEV chuyên dụng.
//!
//! ## Flows
//! - **Trading Flow**: 
//!   - API/User -> ManualTradeExecutor -> chain_adapters -> blockchain
//!   - SmartTradeExecutor -> analys (token/risk) -> chain_adapters -> blockchain
//!   - MevBotImpl -> analys (mempool/token/risk) -> chain_adapters -> blockchain
//! - **Data Flow**:
//!   - analys (mempool/token/risk) -> tradelogic -> execution
//!   - chain_adapters -> tradelogic -> decision -> chain_adapters
//! - **Coordination Flow**:
//!   - tradelogic.executor -> GLOBAL_COORDINATOR -> tradelogic.executor
//!   - mev_logic -> GLOBAL_COORDINATOR -> smart_trade
//!
//! # Mối liên hệ
//! - **lib.rs**: Trung tâm điều phối qua AppState và GLOBAL_COORDINATOR
//! - **tradelogic** là core, gọi analys để phân tích, gọi chain_adapters để thực thi giao dịch.
//! - **analys** cung cấp phân tích token, mempool, risk cho tradelogic thông qua các API provider.
//! - **chain_adapters** là gateway tới blockchain thực tế.
//! - **smart_trade** sử dụng analys_client để tương tác với API của analys.
//! - **mev_logic** sử dụng 4 provider (mempool, token, risk, opportunity) từ analys/api.
//! - **common/bridge_types** được sử dụng và re-export bởi chain_adapters làm standard interface.
