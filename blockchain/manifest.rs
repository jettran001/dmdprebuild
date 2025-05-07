//! 🧭 Entry Point: Đây là manifest chính chứa toàn bộ module của dự án blockchain.
//! Cấu trúc chính: `onchain`, `src/sdk`, `src/processor`, `src/migrations`.
//! Bot hãy bắt đầu từ đây để resolve module path chính xác.
//! Được dùng làm tài liệu tham chiếu khi import từ domain khác (ví dụ: wallet, snipebot).

/*
===========================================================================================
MỐI LIÊN HỆ MODULE & API ROUTES - blockchain/onchain
===========================================================================================
- bridge_interface.sol: Router trung tâm, import/export với erc20_wrappeddmd.sol, erc1155_wrapper.sol, erc1155_bridge_adapter.sol, erc1155_unwrapper_near.rs, ERC20BridgeReceiverNear.rs, interfaces/BridgeTypes.sol, libraries/BridgePayloadCodec.sol, libraries/ChainRegistry.sol.
- erc20_wrappeddmd.sol: Được gọi bởi bridge_interface.sol, erc1155_wrapper.sol. Import interfaces/BridgeTypes.sol.
- erc1155_wrapper.sol: Gọi tới bridge_interface.sol, erc20_wrappeddmd.sol. Import interfaces/BridgeTypes.sol.
- erc1155_unwrapper_near.rs: Nhận payload từ bridge_interface.sol, gửi event về bridge_interface.sol.
- ERC20BridgeReceiverNear.rs: Nhận payload từ bridge_interface.sol, gửi event về bridge_interface.sol.
- erc1155_bridge_adapter.sol: Được gọi bởi bridge_interface.sol, import interfaces/BridgeTypes.sol.
- interfaces/BridgeTypes.sol: Được import bởi bridge_interface.sol, erc20_wrappeddmd.sol, erc1155_wrapper.sol, erc1155_bridge_adapter.sol.
- libraries/BridgePayloadCodec.sol: Được import bởi bridge_interface.sol, erc1155_bridge_adapter.sol.
- libraries/ChainRegistry.sol: Được import bởi bridge_interface.sol.

- dmd_bsc_contract.sol: Được gọi bởi erc1155_wrapper.sol, có thể import bridge_interface.sol.
- near_contract/smartcontract.rs: Nhận/gửi payload với bridge_interface.sol, erc20_wrappeddmd.sol.
- solana_contract/smartcontract.rs: Nhận/gửi payload với bridge_interface.sol, erc20_wrappeddmd.sol.

API/public function chính:
- bridgeToken, bridgeNative, wrap, unwrap, wrapAndBridge, wrapAndBridgeWithUnwrap, mintWrapped, burnWrapped, receive_from_chain, bridge_to_chain, get_bridge_transactions, get_bridge_status, get_bridge_config, estimateFee, pause/unpause, emergencyWithdraw, rescueStuckToken, cleanup, batch, registerUnwrapper, getUnwrapper, unwrap registry, mapping wrapped token.

Các route chính (qua LayerZero hoặc WASM API):
- /bridge/bridgeToken
- /bridge/bridgeNative
- /bridge/estimateFee
- /bridge/getBridgeConfig
- /bridge/getSupportedTokens
- /bridge/wrapAndBridge
- /bridge/wrapAndBridgeWithUnwrap
- /bridge/unwrap
- /bridge/emergencyWithdraw
- /bridge/rescueStuckToken
- /bridge/cleanup
- /bridge/batch
- /bridge/registerUnwrapper
- /bridge/getUnwrapper
- /bridge/getBridgeTransactions
- /bridge/getBridgeStatus
- /bridge/getBridgeAttemptInfo

Luồng import/export:
- bridge_interface.sol là trung tâm, các contract khác import/export dữ liệu qua nó.
- Các contract chain khác (NEAR, Solana) nhận/gửi payload với bridge_interface.sol qua LayerZero.
- Các thư viện và interface được import vào các contract chính để chuẩn hóa type, event, payload.
===========================================================================================
*/

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
 * - Ngôn ngữ: Solidity 0.8.20 cho EVM chains, Rust cho NEAR và Solana
 * - Cấu trúc chính:
 *   - onchain/smartcontract/: Smart contracts cho các blockchain
 *   - onchain/bridge/: Logic bridge qua LayerZero theo luồng 
 *     DMD ERC1155 → wrap → DMD ERC20 → LayerZero → DMD NEAR (auto unwrap)
 *     DMD NEAR → LayerZero → DMD ERC20 → unwrap → DMD ERC1155
 *     Hỗ trợ bridge native token, ERC20, ERC1155, ERC721, ERC777, unwrap registry, auto unwrap, multi-token, mapping unwrap/wrapped token cho từng chain, daily limit, event chuẩn hóa.
 * - Nhiệm vụ: 
 *   - Tạo các smart contract cho các blockchain khác nhau
 *   - Xây dựng chain adapter
 *   - Phát triển bridge logic qua layerzero
 *   - Sau khi tạo xong sẽ được tách riêng ra khỏi hệ thống
 * - Lưu ý quan trọng:
 *   - Tất cả smart contract phải sử dụng Solidity phiên bản 0.8.20
 *   - Sử dụng chuẩn ERC-1155 cho DMD Token và ERC-20 cho Wrapped DMD
 * 
 * 2. SDK (blockchain/src/sdk)
 * --------------------------
 * - Mô tả: Cung cấp hàm Rust → WASM (ví dụ: transfer_dmd, decode_event)
 * - Đặc điểm: Sạch, đơn giản, ổn định, có thể compile sang WASM
 * - Cấu trúc chính:
 *   - sdk/api.rs: REST API endpoints cho frontend
 *   - sdk/bridge_client.rs: Client cho bridge API
 *   - Các chức năng khác:
 *     - Token API: Tích hợp trong bridge_orchestrator.rs và api.rs
 *     - Wallet API: Được kết nối qua domain wallet/src/walletmanager/api.rs
 *     - WASM: Được kết nối qua domain network/src/wasm  
 * - Nhiệm vụ:
 *   - Cung cấp giao diện đơn giản cho việc tương tác với blockchain
 *   - Expose SDK cho frontend thông qua wasm-bindgen
 *   - Cung cấp API cho ứng dụng frontend
 *   - Tạo các client cho bridge API  
 * 
 * 3. PROCESSOR (blockchain/src/processor)
 * --------------------------------------
 * - Mô tả: Theo dõi trạng thái và xử lý các giao dịch cross-chain
 * - Đặc điểm: Xử lý tác vụ phức tạp, tự động hóa quy trình
 * - Cấu trúc chính:
 *   - processor/bridge_orchestrator.rs: Điều phối giao dịch cross-chain
 *   - processor/layerzero.rs: Client tương tác với LayerZero protocol
 *   - processor/wormhole.rs: Client tương tác với Wormhole protocol
 *   - processor/mod.rs: Module definitions cho processor
 * - Nhiệm vụ:
 *   - Điều phối cross-chain bridge
 *   - Theo dõi trạng thái giao dịch
 *   - Xử lý và relay các bridge transaction
 *   - Theo dõi và retry các giao dịch thất bại
 *   - Ước tính phí bridge giữa các blockchain
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
    │   │   ├── dmd_bsc_contract.sol -> Smart contract DMD trên BSC (Solidity 0.8.20)
    │   │   ├── near_contract/      -> Smart contract cho NEAR
    │   │   │   ├── smartcontract.rs -> Implementation contract NEAR
    │   │   │   ├── mod.rs          -> Module definitions cho NEAR contract
    │   │   ├── solana_contract/    -> Smart contract cho Solana
    │   │   │   ├── smartcontract.rs -> Implementation contract Solana
    │   │   │   ├── mod.rs          -> Module definitions cho Solana contract
    │   │   ├── bridge/                 -> Logic bridge qua LayerZero/Wormhole
    │   │   │   ├── bridge_interface.sol       -> Router trung tâm, kế thừa NonblockingLzApp của LayerZero
    │   │   ├── erc20_wrappeddmd.sol       -> ERC-20 đại diện trong quá trình bridge
    │   │   ├── erc1155_wrapper.sol        -> Wrapper cho ERC-1155 thành ERC-20
    │   │   ├── erc1155_unwrapper_near.rs  -> Bộ unwrap/bridge trên NEAR
    │   │   ├── erc1155_bridge_adapter.sol -> Adapter kết nối với bridge protocol (deprecated)
    │   │   ├── ERC20BridgeReceiverNear.rs -> Bridge receiver cho NEAR
    │   │   ├── README.md                  -> Tài liệu hướng dẫn sử dụng bridge system
    │   │   ├── TESTING.md                 -> Tài liệu test coverage bridge
    │   │   ├── SECURITY_AUDIT.md          -> Báo cáo audit bảo mật bridge
    │   │   ├── interfaces/                -> Interface chuẩn hóa cho bridge
    │   │   │   ├── BridgeTypes.sol
    │   │   ├── libraries/                 -> Thư viện dùng chung cho bridge
    │   │   │   ├── ChainRegistry.sol
    │   │   │   ├── BridgePayloadCodec.sol
    │   │   └── (các file khác nếu có)
    ├── src/                        -> Source code chính của dự án
    │   ├── lib.rs                  -> Entry point cho crate library
    │   ├── mod.rs                  -> Module definitions
    │   ├── main.rs                 -> Entry point cho backend service
    │   ├── migrations/             -> Database migrations
    │   │   ├── create_bridge_transactions.sql -> Schema bridge transactions
    │   │   ├── mod.rs              -> Helper functions để chạy migrations
    │   ├── sdk/                    -> API Rust → WASM
    │   │   ├── api.rs              -> REST API endpoints
    │   │   ├── bridge_client.rs    -> Client cho bridge API
    │   │   ├── mod.rs              -> Module definitions
    │   ├── processor/              -> Xử lý logic backend
    │   │   ├── bridge_orchestrator.rs -> Điều phối giao dịch cross-chain
    │   │   ├── layerzero.rs        -> Client tương tác với LayerZero protocol
    │   │   ├── wormhole.rs         -> Client tương tác với Wormhole protocol
    │   │   ├── mod.rs              -> Module definitions
    └── migrations/              -> Database migrations
*/

// Mối liên kết:
// - onchain/smartcontract chứa mã nguồn smart contract cho các blockchain khác nhau
// - Processor tương tác với onchain thông qua SDK
// - SDK cung cấp interface cho frontend
// - Migrations quản lý schema cho database và được sử dụng trong main.rs
// - Wallet API được kết nối qua domain wallet/src/walletmanager/api.rs
// - WASM API được kết nối qua domain network/src/wasm

// Kết nối giữa các domain:
// - Processor là tầng trung gian xử lý backend cho blockchain và các domain khác, bao gồm wallet và snipebot
// - SDK là nơi xuất API ra frontend (domain common) và các ứng dụng khác
// - Domain common cũng là frontend, kết nối trực tiếp với SDK thông qua các API đã được xuất
// - Domain wallet kết nối với processor thông qua wallet/src/walletmanager/api.rs
// - Domain network kết nối với SDK thông qua network/src/wasm

// Kết nối giữa Bridge, Smartcontract và Processor:
// - Bridge và Smartcontract (onchain) đã được kết nối với Processor thông qua bridge_orchestrator.rs
// - Bridge_orchestrator lắng nghe và xử lý các sự kiện từ smart contract thông qua provider (BSC, NEAR, Solana)
// - Bridge_orchestrator điều phối các giao dịch cross-chain qua LayerZero và Wormhole
// - Processor theo dõi trạng thái giao dịch và retry khi cần thiết
// - SDK giao tiếp với bridge_orchestrator thông qua api.rs và bridge_client.rs
// - Frontend (domain common) truy cập các API này thông qua bridge_client

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
            pub const LAYERZERO_DIRECT_CONNECTION: &str = "layerzero_direct_connection";
            pub const ERC1155_WRAPPER: &str = "erc1155_wrapper";
            pub const ADDRESS_VALIDATION: &str = "address_validation";
            pub const WRAPPED_DMD_TOKEN: &str = "wrapped_dmd_token";
        }
        
        pub mod bridge_flow {
            pub const DMD_BRIDGE_FLOW: &str = "DMD ERC1155 → wrap → DMD ERC20 → LayerZero → DMD NEAR (auto unwrap) → LayerZero → DMD ERC20 → unwrap → DMD ERC1155";
            pub const BRIDGE_COMPONENTS: &[&str] = &[
                "bridge_interface.sol - Router trung tâm, hỗ trợ multi-token (ERC20, ERC1155, ERC721, ERC777, native), unwrap registry, auto unwrap, quản lý daily limit, mapping unwrap/wrapped token cho từng chain, event chuẩn hóa.\n\
                \nAPI chính:\n\
                - bridgeToken(token, tokenId, amount, dstChainId, toAddress, needUnwrap, adapterParams): Bridge bất kỳ token nào (ERC20, ERC1155, ERC721, ERC777, native)\n\
                - bridgeNative(dstChainId, toAddress, adapterParams): Bridge native token\n\
                - addWhitelistedToken(token, tokenType), removeWhitelistedToken(token): Quản lý whitelist token\n\
                - updateWrappedTokenForChain(chainId, token), getWrappedTokenForChain(chainId): Mapping wrapped token cho từng chain\n\
                - registerUnwrapper(chainId, unwrapper), getUnwrapper(chainId): Registry unwrapper cho auto unwrap\n\
                - getSupportedTokens(): Lấy danh sách token hỗ trợ\n\
                - estimateFee(dstChainId, amount): Ước tính phí bridge\n\
                - updateBridgeConfig(chainId, dailyLimit, transactionLimit, dailyUnwrapLimit, baseFee, feePercentage, feeCollector): Cập nhật cấu hình bridge theo chain\n\
                - getBridgeConfig(chainId): Lấy thông tin cấu hình bridge theo chain\n\
                - pause(), unpause(): Dừng/mở bridge\n\
                - Các hàm cleanup, batch, emergency withdraw, rescue stuck token, timelock\n\
                \nCác chuẩn hóa đã hoàn thành:\n\
                1. Chuẩn hóa định nghĩa struct, interface, event, status (interfaces/BridgeTypes.sol)\n\
                2. Đồng bộ định dạng payload cross-chain với version và checksum (libraries/BridgePayloadCodec.sol)\n\
                3. Đồng bộ chainId, mapping, registry (libraries/ChainRegistry.sol)\n\
                4. Kiểm tra và lưu nonce cross-chain để chống replay\n\
                5. Kiểm tra và validate địa chỉ cross-chain\n\
                6. Đồng bộ logic auto-unwrap, unwrapper registry\n\
                7. Đồng bộ rescue, stuck, emergency với event chuẩn\n\
                8. Đồng bộ limit, rate, fee (struct BridgeConfig, mapping bridgeConfigs[chainId])\n\
                9. Chuẩn hóa timelock cho các thao tác nhạy cảm\n\
                10. Thêm version byte đầu và checksum cuối vào payload\n\
                \nEvent chuẩn hóa:\n\
                - BridgeInitiated, BridgeReceived, BridgeStatusUpdated, BridgeOperationCreated, BridgeOperationUpdated, TokenWrapped, TokenUnwrapped, AutoUnwrapProcessed, FailedTransactionStored, RetryFailed, FeeUpdated, TrustedRemoteUpdated, TokenTypeSupported, EmergencyWithdrawal, UnwrapperRegistered, UnwrapperRemoved, BridgeOperationsCleaned, BridgeConfigUpdated, StuckTokenRescued, StuckERC1155Rescued, StuckETHRescued\n\
                \nFlow cụ thể:\n\
                1. User gọi bridgeToken/bridgeNative → contract kiểm tra loại token, daily limit, fee, trustedRemote, lưu trạng thái, emit event.\n\
                2. Gửi payload (kèm version byte và checksum) qua LayerZero, contract đích nhận, kiểm tra trustedRemote, decode payload, lưu trạng thái, emit event.\n\
                3. Nếu là wrapped token và có unwrapper registry, tự động gọi unwrap (auto unwrap) → kiểm tra rate limit, allowance, gọi unwrapper, emit event.\n\
                4. Toàn bộ trạng thái bridge operation, unwrap, failed tx đều được lưu và emit event để tracking/audit.\n\
                5. Hỗ trợ retry, cleanup, batch, emergency withdraw, rescue stuck token, timelock, registry unwrap, mapping wrapped token cho từng chain.\n\
                6. Chuẩn hóa event, custom error, revert rõ ràng, đồng bộ với các contract NEAR/EVM khác.\n\
                ",
                "libraries/BridgePayloadCodec.sol - Library chuẩn hóa encode/decode BridgePayload, thêm version byte đầu và checksum cuối, kiểm tra tính toàn vẹn payload",
                "libraries/ChainRegistry.sol - Library chuẩn hóa mapping chainId <-> chainName <-> remoteAddress <-> wrapped token <-> unwrapper",
                "interfaces/BridgeTypes.sol - Định nghĩa các type, enum, struct, event chuẩn hóa cho toàn bộ bridge system",
                "erc1155_wrapper.sol - Wrap/unwrap giữa DMD ERC-1155 và DMD ERC-20, hỗ trợ wrapAndBridgeWithUnwrap",
                "erc20_wrappeddmd.sol - ERC-20 token cho bridge, quản lý quyền mint/burn, kiểm tra proxy, hỗ trợ multi-chain",
                "erc1155_unwrapper_near.rs - Bộ unwrap/bridge trên NEAR, đồng bộ event và payload với bridge_interface.sol"
            ];
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
    
    // Solidity & Smart Contract Dependencies
    pub const SOLIDITY: &str = "solidity:0.8.20"; // Solidity compiler version
    pub const OPENZEPPELIN: &str = "openzeppelin:5.0.0"; // OpenZeppelin contracts version
    
    // Bridge và Cross-Chain Messaging
    pub const LAYERZERO: &str = "layerzero:0.8.0"; // LayerZero cross-chain messaging
    pub const WORMHOLE: &str = "wormhole:0.9.0"; // Wormhole cross-chain messaging
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

/// --- BRIDGE MODULES ---
/// 
/// Bridge Module cho Diamond Chain là hệ thống bridge đơn giản sử dụng LayerZero trực tiếp,
/// cho phép kết nối nhiều blockchain khác nhau (NEAR, BSC, Ethereum, Solana...).
///
/// Luồng hoạt động của Bridge DMD:
///
/// 1. User sở hữu token DMD chuẩn ERC-1155
///    - DMD là token đa mục đích (NFT + FT), token chính trên BSC
///    - Smart contract chính là DiamondToken.sol
///
/// 2. Wrapper ERC-1155Wrapper.sol
///    - Giao diện cho phép wrap ERC-1155 → ERC-20 thành ERC20WrappedDMD.sol
///    - Ví dụ: mỗi 1000 DMD => 1000 wrapped ERC-20 DMD, giữ lại balance user
///
/// 3. Token trung gian ERC20WrappedDMD.sol
///    - Là contract dạng ERC-20, có tổng cung phản ánh các token đã wrap
///    - Là token sẽ được dùng để gửi qua bridge LayerZero
///
/// 4. Bridge thông qua:
///    - DiamondBridgeInterface.sol: Quản lý logic gửi và nhận qua LayerZero
///    - Kế thừa trực tiếp từ NonblockingLzApp của LayerZero
///
/// 5. Bridge từ BSC sang NEAR
///    - Gửi payload đến contract đích trên NEAR (địa chỉ được mapping qua trustedRemote)
///    - Payload gồm: token_id, amount, user, destination_chain_id
///    - DiamondBridgeInterface phát event BridgeInitiated
///
/// 6. Tại NEAR
///    - ERC20BridgeReceiverNear.rs: Contract Rust, mint wrapped token tại NEAR
///    - Xác nhận payload hợp lệ, giải mã địa chỉ, mint đúng số lượng cho user
///
/// 7. Từ NEAR về BSC
///    - User sở hữu Wrapped DMD tại NEAR (được mint từ lần bridge trước)
///    - Contract ERC1155UnwrapperNear.rs cho phép burn wrapped token tại NEAR
///    - Gửi payload ngược về BSC thông qua LayerZero (vẫn giữ định dạng đã dùng)
///
/// 8. DiamondBridgeInterface tại BSC
///    - Nhận event từ LayerZero
///    - Gọi unwrap() từ ERC20WrappedDMD.sol → burn wrapped token
///    - Mint lại ERC-1155 gốc cho user thông qua DiamondToken.sol
///
/// Danh sách file cần thiết:
/// ```
/// | File                     | Chain | Vai trò                         |
/// |--------------------------|-------|--------------------------------|
/// | DiamondToken.sol         | BSC   | Token chính ERC-1155            |
/// | ERC1155Wrapper.sol       | BSC   | Chuyển đổi 1155 → ERC20        |
/// | ERC20WrappedDMD.sol      | BSC   | Token trung gian để bridge     |
/// | DiamondBridgeInterface.sol | BSC   | Router điều phối logic gửi/nhận |
/// | ERC20BridgeReceiverNear.rs | NEAR  | Nhận wrapped ERC20, mint       |
/// | ERC1155UnwrapperNear.rs  | NEAR  | Gửi ngược về lại BSC           |
/// ```
///
/// Trusted Remote trong Bridge:
/// - Khái niệm: setTrustedRemote() là hàm trong NonblockingLzApp.sol của LayerZero dùng để:
///   + Thiết lập niềm tin giữa hai contract đang giao tiếp qua LayerZero
///   + Tránh các lỗ hổng giả mạo từ chain khác / contract lạ
///   + Mỗi cặp contract cross-chain phải được cả 2 phía set thủ công
///
/// - Cơ chế hoạt động:
///   ```solidity
///   function setTrustedRemote(uint16 _chainId, bytes calldata _remoteAddress) external onlyOwner {
///       trustedRemotes[_chainId] = _remoteAddress;
///   }
///   ```
///
/// - Ví dụ:
///   + Từ BSC: DiamondBridgeInterface.sol gọi setTrustedRemote(115, near_contract_bytes)
///   + Từ NEAR: ERC20BridgeReceiverNear.rs lưu contract BSC trong mapping đối ứng
///   + Cả 2 bên phải biết và tin tưởng địa chỉ của nhau → gọi mới thành công
///
/// - Payload validation:
///   + Khi nhận message từ LayerZero, _nonblockingLzReceive() sẽ check:
///   + require(keccak256(_srcAddress) == keccak256(trustedRemotes[_srcChainId]), "Not trusted remote");
///
/// Cấu trúc thư mục:
/// ```
/// brigde/
/// ├── bridge_interface.sol       # Router trung tâm, kế thừa NonblockingLzApp của LayerZero
/// ├── erc20_wrappeddmd.sol       # ERC-20 đại diện trong quá trình bridge
/// ├── erc1155_wrapper.sol        # Wrapper cho ERC-1155 thành ERC-20
/// ├── erc1155_unwrapper_near.rs  # Bộ giải nén trên NEAR
/// ├── erc1155_bridge_adapter.sol # Adapter kết nối với bridge protocol
/// ├── BridgeDeployer.sol         # Factory contract để triển khai bridge system
/// ├── README.md                  # Tài liệu hướng dẫn sử dụng bridge system
/// ```
/// 
/// Mô tả chi tiết các thành phần bridge:
/// 
/// 1. bridge_interface.sol — Router trung tâm
///    - Kế thừa trực tiếp từ NonblockingLzApp của LayerZero
///    - Quản lý trustedRemotes cho bảo mật cross-chain
///    - Xử lý gửi và nhận token thông qua bridge
///    - Tích hợp tính năng pause/unpause và fee collection
///
/// 2. erc20_wrappeddmd.sol — ERC-20 đại diện
///    - Contract chuẩn ERC-20 đại diện cho DMD khi ở giữa bridge
///    - Có thể mint/burn từ bridge interface hoặc wrapper
///    - Được sử dụng như "hàng hóa trung chuyển" trong quá trình bridge
///
/// 3. erc1155_wrapper.sol — Bộ đóng gói (wrap)
///    - Nhận ERC-1155 từ user
///    - Lock hoặc burn nó (tuỳ thiết kế)
///    - Mint ra một ERC-20 wrapped đại diện
///
/// 4. erc1155_unwrapper_near.rs — Bộ giải nén trên NEAR
///    - Nhận payload gửi từ bridge
///    - Burn wrapped token tại NEAR
///    - Gửi payload ngược về BSC để mint lại token gốc
///
/// 5. erc1155_bridge_adapter.sol — Adapter với bridge protocol
///    - Kết nối giữa wrapper và bridge interface
///    - Xử lý chuyển đổi format dữ liệu giữa các hệ thống
///
/// 6. BridgeDeployer.sol — Factory triển khai
///    - Triển khai và liên kết các thành phần bridge
///    - Quản lý mối quan hệ giữa các contracts trong hệ thống

///
/// =============================
/// CÁC CẢI TIẾN MỚI NHẤT (2024-08)
/// =============================
/// - Chuẩn hóa event, custom error, revert rõ ràng, audit trail cho toàn bộ bridge.
/// - Thêm rate limit per tx, per day, per address, unwrap limit, unwrapId unique, memory cleanup.
/// - Multi-sig cho các thao tác nhạy cảm, timelock, emergency withdraw, proxy permission.
/// - Retry/auto-retry cho failed transaction, tracking trạng thái, event chi tiết.
/// - Xác thực trusted remote, signature, state root, confirmation, emergency, rollback.
/// - Quản lý stuck token, cleanup, batch, cảnh báo out-of-gas, memory bloat.
/// - Chuẩn hóa interface, event, payload, đồng bộ EVM/NEAR, LayerZero/Wormhole.
/// - Đã fix hầu hết các lỗi nghiêm trọng, các lỗi tiềm ẩn đã được liệt kê và có hướng xử lý rõ ràng.
/// - Đảm bảo mọi thay đổi đều được cập nhật vào manifest, .bugs và tài liệu kỹ thuật.


