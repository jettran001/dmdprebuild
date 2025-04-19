//! 🧭 Entry Point: Đây là manifest chính chứa toàn bộ module của dự án blockchain.
//! Cấu trúc chính: `onchain`, `src/sdk`, `src/processor`, `src/migrations`.
//! Bot hãy bắt đầu từ đây để resolve module path chính xác.
//! Được dùng làm tài liệu tham chiếu khi import từ domain khác (ví dụ: wallet, snipebot).

/**
 * ===========================================================================================
 * CẤU TRÚC DOMAIN BLOCKCHAIN
 * ===========================================================================================
 * 
 * Cấu trúc của blockchain được tổ chức thành 4 phần chính:
 * 
 * 1. ONCHAIN (blockchain/onchain)
 * --------------------------------------
 * - Mô tả: Nơi tạo các smartcontract, chain adapter, bridge logic qua layerzero
 * - Ngôn ngữ: Solidity cho EVM chains, Rust cho NEAR và Solana
 * - Cấu trúc chính:
 *   - onchain/smartcontract/: Smart contracts cho các blockchain
 *   - onchain/brigde/: Logic bridge qua LayerZero
 * - Nhiệm vụ: 
 *   - Tạo các smart contract cho các blockchain khác nhau
 *   - Xây dựng chain adapter
 *   - Phát triển bridge logic qua layerzero
 *   - Sau khi tạo xong sẽ được tách riêng ra khỏi hệ thống
 * 
 * 2. SDK (blockchain/src/sdk)
 * --------------------------
 * - Mô tả: Cung cấp hàm Rust → WASM (ví dụ: transfer_dmd, decode_event)
 * - Đặc điểm: Sạch, đơn giản, ổn định, có thể compile sang WASM
 * - Cấu trúc chính:
 *   - sdk/token/: API cho token (balance, transfer, token info)
 *   - sdk/bridge/: API cho bridge (bridge transaction, status, fee)
 *   - sdk/wallet/: API cho quản lý ví (account, keys, signature)
 *   - sdk/wasm/: WASM bindings cho frontend
 *   - sdk/api.rs: REST API endpoints cho frontend
 *   - sdk/bridge_client.rs: Client cho bridge API
 * - Nhiệm vụ:
 *   - Cung cấp giao diện đơn giản cho việc tương tác với blockchain
 *   - Expose SDK cho frontend thông qua wasm-bindgen
 *   - Cung cấp API cho ứng dụng frontend
 *   - Tạo các client cho bridge API  
 * 
 * 3. PROCESSOR (blockchain/src/processor)
 * --------------------------------------
 * - Mô tả: Theo dõi trạng thái UX, explorer, dashboard user, Auto-trigger
 * - Đặc điểm: Xử lý tác vụ phức tạp, tự động hóa quy trình
 * - Cấu trúc chính:
 *   - processor/bridge_orchestrator.rs: Điều phối giao dịch cross-chain
 *   - processor/layerzero.rs: Client tương tác với LayerZero protocol
 *   - processor/wormhole.rs: Client tương tác với Wormhole protocol
 *   - processor/explorer/: Tracking và hiển thị trạng thái giao dịch
 *   - processor/dashboard/: Dashboard cho user
 *   - processor/auto_trigger/: Tự động bridge hoặc batch nhiều tx
 * - Nhiệm vụ:
 *   - Điều phối cross-chain bridge
 *   - Theo dõi trạng thái giao dịch
 *   - Cung cấp explorer cho người dùng
 *   - Xây dựng dashboard user
 *   - Tự động bridge hoặc batch nhiều tx
 *
 * 4. MIGRATIONS (blockchain/src/migrations)
 * --------------------------------------
 * - Mô tả: Quản lý schema database và migrations
 * - Đặc điểm: Tự động tạo và cập nhật schema database
 * - Cấu trúc chính:
 *   - migrations/create_bridge_transactions.sql: Schema cho bảng bridge_transactions
 *   - migrations/mod.rs: Helper functions để chạy migrations
 * - Nhiệm vụ:
 *   - Tạo schema database cho bridge transactions
 *   - Quản lý việc cập nhật schema khi có thay đổi
 *   - Tự động chạy migrations khi khởi động ứng dụng
 */

/*
    blockchain/
    ├── Cargo.toml                  -> Cấu hình dependencies
    ├── manifest.rs                 -> Tài liệu tham chiếu module path [liên quan: tất cả các module, BẮT BUỘC đọc đầu tiên]
    ├── onchain/                    -> Nơi phát triển smartcontracts
    │   ├── smartcontract/          -> Smart contracts cho các blockchain
    │   │   ├── dmd_eth.sol         -> Smart contract DMD trên Ethereum
    │   │   ├── dmd_bsc_contract.sol -> Smart contract DMD trên BSC
    │   │   ├── dmd_solana.rs       -> Smart contract DMD trên Solana
    │   │   ├── dmd_near.rs         -> Smart contract DMD trên NEAR
    │   ├── brigde/                 -> Logic bridge qua LayerZero
    │   │   ├── dmd_bsc_bridge.sol  -> Bridge contract trên BSC
    ├── src/                        -> Source code chính của dự án
    │   ├── lib.rs                  -> Entry point cho crate library
    │   ├── mod.rs                  -> Module definitions
    │   ├── main.rs                 -> Entry point cho backend service
    │   ├── migrations/             -> Database migrations
    │   │   ├── create_bridge_transactions.sql -> Schema bridge transactions
    │   │   ├── mod.rs              -> Helper functions cho migrations
    │   ├── sdk/                    -> API Rust → WASM
    │   │   ├── api.rs              -> REST API endpoints
    │   │   ├── bridge_client.rs    -> Client cho bridge API
    │   │   ├── mod.rs              -> Module definitions
    │   │   ├── token/              -> API cho token
    │   │   ├── bridge/             -> API cho bridge
    │   │   ├── wallet/             -> API cho quản lý ví
    │   │   ├── wasm/               -> WASM bindings
    │   ├── processor/              -> Xử lý logic backend
    │   │   ├── bridge_orchestrator.rs -> Điều phối giao dịch cross-chain
    │   │   ├── layerzero.rs        -> Client tương tác với LayerZero
    │   │   ├── wormhole.rs         -> Client tương tác với Wormhole
    │   │   ├── mod.rs              -> Module definitions
    │   │   ├── explorer/           -> Tracking và hiển thị trạng thái giao dịch
    │   │   ├── dashboard/          -> Dashboard cho user
    │   │   ├── auto_trigger/       -> Tự động bridge hoặc batch nhiều tx
*/

// Mối liên kết:
// - onchain/smartcontract chứa mã nguồn smart contract cho các blockchain khác nhau
// - Processor tương tác với onchain thông qua SDK
// - SDK cung cấp interface cho frontend
// - Migrations quản lý schema cho database và được sử dụng trong main.rs

/**
 * ===========================================================================================
 * COMPONENTS
 * ===========================================================================================
 */
pub mod components {
    pub mod bridge {
        pub const VERSION: &str = "0.1.0";
        pub const DESCRIPTION: &str = "Bridge module cho phép chuyển token giữa các blockchain thông qua LayerZero";

        pub struct BridgeManifest;

        pub mod features {
            pub const BRIDGE_EVM_TO_EVM: &str = "bridge_evm_to_evm";
            pub const BRIDGE_EVM_TO_SOLANA: &str = "bridge_evm_to_solana";
            pub const BRIDGE_EVM_TO_NEAR: &str = "bridge_evm_to_near";
            pub const CHECK_TRANSACTION_STATUS: &str = "check_transaction_status";
            pub const MANAGE_ADAPTERS: &str = "manage_adapters";
            pub const CROSS_TRANSACTION_VALIDATION: &str = "cross_transaction_validation";
            pub const ADDRESS_VALIDATION: &str = "address_validation";
            pub const BATCH_TRANSACTION_PROCESSING: &str = "batch_transaction_processing";
        }
    }

    pub mod sdk {
        pub const VERSION: &str = "0.1.0";
        pub const DESCRIPTION: &str = "SDK module cung cấp giao diện Rust có thể compile sang WASM cho frontend";

        pub struct SdkManifest;

        pub mod features {
            pub const TRANSFER_DMD: &str = "transfer_dmd";
            pub const DECODE_EVENT: &str = "decode_event";
            pub const GET_BALANCE: &str = "get_balance";
            pub const SIGN_TRANSACTION: &str = "sign_transaction";
            pub const CONNECT_WALLET: &str = "connect_wallet";
            pub const ESTIMATE_GAS: &str = "estimate_gas";
            pub const CALCULATE_SLIPPAGE: &str = "calculate_slippage";
            pub const API_CLIENT: &str = "api_client";
            pub const BRIDGE_CLIENT: &str = "bridge_client";
        }
    }

    pub mod processor {
        pub const VERSION: &str = "0.1.0";
        pub const DESCRIPTION: &str = "Processor module xử lý logic backend, theo dõi trạng thái và tự động hóa";

        pub struct ProcessorManifest;

        pub mod features {
            pub const BRIDGE_ORCHESTRATOR: &str = "bridge_orchestrator";
            pub const LAYERZERO_CLIENT: &str = "layerzero_client";
            pub const WORMHOLE_CLIENT: &str = "wormhole_client";
            pub const DASHBOARD_UX: &str = "dashboard_ux";
            pub const EXPLORER: &str = "explorer";
            pub const USER_DASHBOARD: &str = "user_dashboard";
            pub const AUTO_BRIDGE: &str = "auto_bridge";
            pub const BATCH_TRANSACTIONS: &str = "batch_transactions";
            pub const MONITOR_TRANSACTION_STATUS: &str = "monitor_transaction_status";
            pub const RETRY_FAILED_TRANSACTIONS: &str = "retry_failed_transactions";
        }
    }
    
    pub mod migrations {
        pub const VERSION: &str = "0.1.0";
        pub const DESCRIPTION: &str = "Migrations module quản lý schema database và migrations";
        
        pub struct MigrationsManifest;
        
        pub mod features {
            pub const RUN_MIGRATIONS: &str = "run_migrations";
            pub const CHECK_MIGRATION_STATUS: &str = "check_migration_status";
            pub const CREATE_BRIDGE_TRANSACTIONS_TABLE: &str = "create_bridge_transactions_table";
        }
    }
}

/**
 * ===========================================================================================
 * SERVICES
 * ===========================================================================================
 */
pub mod services {
    pub mod transaction {
        pub const VERSION: &str = "0.1.0";
        pub const DESCRIPTION: &str = "Dịch vụ quản lý giao dịch";

        pub struct TransactionManifest;
    }

    pub mod security {
        pub const VERSION: &str = "0.1.0";
        pub const DESCRIPTION: &str = "Dịch vụ bảo mật";

        pub struct SecurityManifest;
    }
    
    pub mod database {
        pub const VERSION: &str = "0.1.0";
        pub const DESCRIPTION: &str = "Dịch vụ quản lý database";
        
        pub struct DatabaseManifest;
    }
}

pub const VERSION: &str = "0.1.0";

/**
 * ===========================================================================================
 * DEPENDENCIES
 * ===========================================================================================
 */
pub mod dependencies {
    pub const CRYPTO: &str = "ring:0.16.20";
    pub const SERDE: &str = "serde:1.0.152";
    pub const WEB3: &str = "web3:0.18.0";
    pub const NEAR_SDK: &str = "near-sdk:4.1.1";
    pub const LOG: &str = "log:0.4.17";
    pub const TOKIO: &str = "tokio:1.25.0";
    pub const ANYHOW: &str = "anyhow:1.0.69";
    pub const WASM_BINDGEN: &str = "wasm-bindgen:0.2.84";
    pub const SQLX: &str = "sqlx:0.6.3";
    pub const ETHERS: &str = "ethers:2.0.7";
    pub const AXUM: &str = "axum:0.6.18";
    pub const REQWEST: &str = "reqwest:0.11.18";
}

/**
 * ===========================================================================================
 * PLANNED FEATURES
 * ===========================================================================================
 */
pub mod planned_features {
    pub mod bridge {
        pub const NEW_BLOCKCHAIN_SUPPORT: &str = "planned:0.2.0";
        pub const ENHANCED_ERROR_HANDLING: &str = "planned:0.2.0";
        pub const CROSS_CHAIN_MESSAGING: &str = "planned:0.2.0";
    }

    pub mod sdk {
        pub const EXTENDED_WASM_BINDINGS: &str = "planned:0.2.0";
        pub const IMPROVED_PERFORMANCE: &str = "planned:0.2.0";
        pub const ADDITIONAL_CHAIN_SUPPORT: &str = "planned:0.2.0";
    }

    pub mod processor {
        pub const ADVANCED_BRIDGE_ORCHESTRATION: &str = "planned:0.2.0";
        pub const ENHANCED_DASHBOARD: &str = "planned:0.2.0";
        pub const SMART_AUTO_TRIGGER: &str = "planned:0.2.0";
    }
    
    pub mod migrations {
        pub const VERSIONED_MIGRATIONS: &str = "planned:0.2.0";
        pub const MIGRATION_ROLLBACK: &str = "planned:0.2.0";
    }
}

/**
 * ===========================================================================================
 * APIS
 * ===========================================================================================
 */
pub mod apis {
    pub mod bridge_api {
        pub const VERSION: &str = "0.1.0";
        pub const BASE_PATH: &str = "/api/v1/bridge";

        pub mod endpoints {
            pub const CREATE_TRANSACTION: &str = "/transaction";
            pub const GET_TRANSACTION_STATUS: &str = "/transaction/:id/status";
            pub const LIST_TRANSACTIONS: &str = "/transactions";
            pub const ESTIMATE_FEE: &str = "/estimate-fee";
            pub const RELAY_TRANSACTION: &str = "/relay";
        }
    }

    pub mod sdk_api {
        pub const VERSION: &str = "0.1.0";
        pub const BASE_PATH: &str = "/api/v1/sdk";

        pub mod endpoints {
            pub const GET_TOKEN_BALANCE: &str = "/token/:address/balance";
            pub const TRANSFER_TOKEN: &str = "/token/transfer";
            pub const ESTIMATE_GAS: &str = "/transaction/estimate-gas";
        }
    }

    pub mod processor_api {
        pub const VERSION: &str = "0.1.0";
        pub const BASE_PATH: &str = "/api/v1/processor";

        pub mod endpoints {
            pub const GET_DASHBOARD: &str = "/dashboard";
            pub const GET_TRANSACTION_HISTORY: &str = "/transactions/history";
            pub const SCHEDULE_AUTO_BRIDGE: &str = "/auto-bridge";
            pub const CREATE_BATCH_TRANSACTION: &str = "/batch-transaction";
        }
    }
}
