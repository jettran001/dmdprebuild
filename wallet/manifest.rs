//! 🧭 Entry Point: Đây là manifest chính chứa toàn bộ module của dự án wallet.
//! Mỗi thư mục là một đơn vị rõ ràng: `walletlogic`, `walletmanager`, `defi`, `users`, `error`.
//! Bot hãy bắt đầu từ đây để resolve module path chính xác.
//! Được dùng làm tài liệu tham chiếu khi import từ domain khác (ví dụ: snipebot).

/*
    wallet/
    ├── Cargo.toml                  -> Cấu hình dependencies
    ├── error.rs                   -> Định nghĩa các lỗi chung cho toàn bộ domain
    ├── lib.rs                     -> Entry point cho thư viện
    ├── main.rs                    -> ứng dụng chạy độc lập
    ├── config.rs                  -> Cấu hình hệ thống và tham số mặc định
    ├── improvement_report.md      -> Báo cáo chi tiết về các lỗi tiềm ẩn, vấn đề cần cải thiện liên quan đến subscription, thread safety, xử lý lỗi blockchain và đề xuất cải tiến module
    ├── cache.rs                   -> Hệ thống cache LRU với TTL, quản lý cache cho user VIP và Premium, xử lý cache thread-safe
    ├── manifest.rs                -> Tài liệu tham chiếu module path [liên quan: tất cả các module, BẮT BUỘC đọc đầu tiên]
    ├── src/walletlogic/           -> Logic cốt lõi ví
    │   ├── mod.rs                 -> Khai báo submodule [liên quan: tất cả các file trong walletlogic]
    │   ├── init.rs                -> Khởi tạo WalletManager, tạo/nhập ví với mã hóa seed/key [liên quan: crypto.rs, handler.rs, utils.rs, bao gồm hàm create_wallet_internal, import_wallet_internal]
    │   ├── handler.rs             -> Xử lý ví, định nghĩa trait WalletManagerHandler với các phương thức quản lý ví, ký/gửi giao dịch, kiểm tra số dư [liên quan: utils.rs, crypto.rs, walletmanager::api, bao gồm export_seed_phrase_internal, export_private_key_internal, remove_wallet_internal, update_chain_id_internal, sign_transaction, send_transaction, get_balance]
    │   ├── utils.rs               -> Các hàm tiện ích, định nghĩa UserType (Free, Premium, VIP), tạo user_id, kiểm tra seed phrase [liên quan: init.rs, handler.rs, bao gồm generate_user_id, generate_default_user_id, is_seed_phrase]
    │   ├── crypto.rs              -> Mã hóa/giải mã seed phrase và private key bằng AES-256-GCM và PBKDF2 [liên quan: init.rs, handler.rs, bao gồm encrypt_data và decrypt_data với 100,000 vòng lặp PBKDF2]
    ├── src/walletmanager/         -> API/UI cho ví, quản lý kết nối blockchain
    │   ├── mod.rs                 -> Khai báo các submodules (api, chain, types, lib) [liên quan: tất cả file trong walletmanager]
    │   ├── api.rs                 -> WalletManagerApi: API công khai để tạo, quản lý, tương tác với ví, gọi vào walletlogic [liên quan: walletlogic::init, walletlogic::handler, types.rs, chain.rs, lib.rs]
    │   ├── chain.rs               -> ChainManager trait và implementations (DefaultChainManager): quản lý kết nối đến các blockchain (EVM, Solana, ...) [liên quan: api.rs, walletlogic::handler, defi::blockchain]
    │   ├── types.rs               -> Các kiểu dữ liệu dùng trong walletmanager (SeedLength, WalletSecret, WalletConfig, WalletInfo) [liên quan: api.rs, chain.rs, lib.rs]
    │   └── lib.rs                 -> Re-export các thành phần chính (WalletManagerApi, WalletConfig, ...) để tiện sử dụng từ bên ngoài [liên quan: api.rs, types.rs]
    ├── src/defi/                  -> Chức năng DeFi (farming, staking, blockchain interaction)
    │   ├── mod.rs                 -> Khai báo và re-export các submodules và thành phần chính của DeFi [liên quan: api.rs, farm.rs, stake.rs, blockchain.rs, contracts.rs, security.rs, error.rs, constants.rs]
    │   ├── api.rs                 -> API công khai DefiApi cho DeFi (get_farming_opportunities, get_staking_opportunities) [liên quan: farm.rs, stake.rs, lib.rs]
    │   ├── lib.rs                 -> Re-export các thành phần chính từ module defi (DefiApi, DefiError, ...) [liên quan: api.rs, farm.rs, stake.rs, error.rs]
    │   ├── farm.rs                -> Logic farming với FarmingManager (add_liquidity, withdraw_liquidity, harvest_rewards) và FarmingOpportunity [liên quan: api.rs, blockchain.rs, contracts.rs]
    │   ├── stake.rs               -> Logic staking với StakingManager (stake_tokens, unstake_tokens, calculate_rewards) và StakingOpportunity [liên quan: api.rs, blockchain.rs, contracts.rs]
    │   ├── blockchain.rs          -> Định nghĩa interface BlockchainProvider, BlockchainConfig, BlockchainType và các hàm factory để tạo provider [liên quan: chain.rs, error.rs, constants.rs, blockchain/]
    │   ├── contracts.rs           -> Tương tác với các smart contracts DeFi (Pools, StakingContracts) [liên quan: farm.rs, stake.rs, blockchain.rs]
    │   ├── chain.rs               -> Định nghĩa ChainId và các thông tin liên quan đến chain (RPC URLs, explorer URLs) [liên quan: blockchain.rs]
    │   ├── security.rs            -> Các biện pháp bảo mật cho DeFi (validation, rate limiting, risk assessment) [liên quan: farm.rs, stake.rs, contracts.rs]
    │   ├── error.rs               -> Định nghĩa DefiError cho module DeFi [liên quan: tất cả các file trong defi]
    │   ├── constants.rs           -> Các hằng số sử dụng trong module DeFi (timeouts, retries, default values) [liên quan: blockchain.rs, farm.rs, stake.rs]
    │   ├── blockchain/            -> Implementations cho các blockchain providers
    │   │   ├── mod.rs             -> (Có thể cần nếu có logic chung cho providers)
    │   │   └── non_evm/           -> Providers cho các blockchain không tương thích EVM
    │   │       ├── mod.rs         -> Khai báo các non-EVM provider modules
    │   │       ├── solana.rs      -> Solana provider [liên quan: blockchain.rs]
    │   │       ├── tron.rs        -> Tron provider [liên quan: blockchain.rs]
    │   │       ├── hedera.rs      -> Hedera provider [liên quan: blockchain.rs]
    │   │       ├── cosmos.rs      -> Cosmos provider [liên quan: blockchain.rs]
    │   │       ├── near.rs        -> NEAR provider [liên quan: blockchain.rs]
    │   │       └── diamond.rs     -> Diamond provider [liên quan: blockchain.rs]
    │   ├── tests.rs               -> Unit tests cho các chức năng DeFi (farm, stake, blockchain interaction) [liên quan: farm.rs, stake.rs, blockchain.rs]
    │   └── tests/                 -> Integration tests cho DeFi
    │       └── integration.rs     -> Các integration tests cho flow DeFi hoàn chỉnh
    ├── src/users/                 -> Quản lý người dùng (free, premium, VIP) và đăng ký
    │   ├── mod.rs                 -> Khai báo và re-export các submodules (free_user, subscription, premium_user, vip_user) [liên quan: tất cả file và thư mục trong users]
    │   ├── free_user/             -> Logic cho người dùng miễn phí
    │   │   ├── mod.rs             -> Khai báo các modules con (types, manager, auth, limits, records, queries, test_utils) [liên quan: tất cả file trong free_user]
    │   │   ├── types.rs           -> Định nghĩa kiểu dữ liệu FreeUserData, UserStatus, TransactionType [liên quan: manager.rs, auth.rs, limits.rs]
    │   │   ├── manager.rs         -> Quản lý FreeUserManager, sử dụng cache [liên quan: types.rs, auth.rs, limits.rs, records.rs, queries.rs, cache.rs]
    │   │   ├── auth.rs            -> Xác thực người dùng miễn phí (login, register, verify) [liên quan: types.rs, manager.rs]
    │   │   ├── limits.rs          -> Kiểm tra giới hạn giao dịch [liên quan: types.rs, manager.rs]
    │   │   ├── records.rs         -> Ghi nhận hoạt động [liên quan: types.rs, manager.rs]
    │   │   ├── queries.rs         -> Truy vấn dữ liệu người dùng qua cache [liên quan: types.rs, manager.rs, cache.rs]
    │   │   └── test_utils.rs      -> Công cụ kiểm thử với MockWalletHandler [liên quan: tests/auth_tests.rs]
    │   ├── subscription/          -> Logic quản lý đăng ký, thanh toán, staking, NFT, auto-trade
    │   │   ├── mod.rs             -> Khai báo và re-export các submodules và types chính [liên quan: tất cả file trong subscription]
    │   │   ├── manager.rs         -> SubscriptionManager: điều phối chính, xử lý nâng/hạ cấp, xác minh [liên quan: user_subscription.rs, types.rs, auto_trade.rs, nft.rs, staking.rs, payment.rs, events.rs, walletmanager::api, vip.rs]
    │   │   ├── user_subscription.rs -> Định nghĩa UserSubscription, SubscriptionConverter [liên quan: manager.rs, types.rs, auto_trade.rs, nft.rs]
    │   │   ├── types.rs           -> Định nghĩa các kiểu dữ liệu SubscriptionType, SubscriptionStatus, Feature, PaymentToken [liên quan: manager.rs, user_subscription.rs, staking.rs]
    │   │   ├── constants.rs       -> Hằng số (giá, stake amounts, APY) [liên quan: manager.rs, staking.rs, payment.rs]
    │   │   ├── auto_trade.rs      -> AutoTradeManager: quản lý thời gian auto-trade, sử dụng cache [liên quan: manager.rs, user_subscription.rs, cache.rs]
    │   │   ├── nft.rs             -> Quản lý NFT cho VIP (VipNftInfo, NonNftVipStatus), kiểm tra sở hữu [liên quan: manager.rs, walletmanager::api, vip_user.rs]
    │   │   ├── staking.rs         -> Quản lý staking DMD token (ERC-1155), TokenStake, StakeStatus [liên quan: manager.rs, types.rs, constants.rs, walletmanager::api]
    │   │   ├── payment.rs         -> Xử lý thanh toán, xác minh giao dịch blockchain [liên quan: manager.rs, constants.rs, walletmanager::api]
    │   │   ├── vip.rs             -> Logic đặc biệt cho VIP liên quan đến subscription [liên quan: manager.rs]
    │   │   ├── events.rs          -> Định nghĩa và phát sự kiện SubscriptionEvent, EventType, EventEmitter [liên quan: manager.rs, user_subscription.rs]
    │   │   ├── utils.rs           -> Các hàm tiện ích cho subscription [liên quan: tất cả file trong subscription]
    │   │   └── tests.rs           -> Unit tests cho các chức năng subscription [liên quan: tất cả file trong subscription]
    │   ├── premium_user.rs        -> Logic người dùng premium (PremiumUserData, PremiumUserManager) [liên quan: subscription::manager, walletmanager::api]
    │   ├── vip_user.rs            -> Logic người dùng VIP (VipUserData, VipUserManager), quản lý NFT/staking [liên quan: subscription::manager, subscription::nft, subscription::staking, walletmanager::api]
    │   └── subscription.rs        -> (Sắp di chuyển sang thư mục subscription) [liên quan: premium_user.rs, vip_user.rs]
    ├── src/contracts/             -> Module tương tác với smart contracts
    │   ├── mod.rs                 -> Khai báo submodule và các hàm chung [liên quan: erc20.rs, walletmanager::api]
    │   ├── erc20.rs               -> Các hàm tương tác với ERC-20 contracts (get_balance, approve, transfer) [liên quan: mod.rs, walletmanager::api]
    ├── tests/                     -> Integration tests cho wallet
    │   ├── mod.rs                 -> Khai báo các test modules cho free_user [liên quan: auth_tests.rs, limits_tests.rs, records_tests.rs]
    │   ├── integration.rs         -> Integration tests cho wallet lifecycle, import/export, sign/balance [liên quan: walletmanager::api, walletlogic, config]
    │   ├── auth_tests.rs          -> Tests cho xác thực người dùng, test_verify_free_user [liên quan: users::free_user::auth, free_user::test_utils]
    │   ├── limits_tests.rs        -> Tests cho kiểm tra giới hạn giao dịch [liên quan: users::free_user::limits]
    │   ├── records_tests.rs       -> Tests cho ghi nhận hoạt động [liên quan: users::free_user::records]
    │   └── cache_tests.rs         -> Kiểm thử hệ thống cache

    Mối liên kết:
    - walletlogic phụ thuộc error (dùng WalletError)
    - walletlogic::crypto được dùng bởi init.rs để mã hóa khóa và bởi handler.rs để giải mã
    - walletlogic::init quản lý WalletManager với wallets: Arc<RwLock<HashMap<Address, WalletInfo>>>
    - walletlogic::init thực hiện tạo và nhập ví, sử dụng utils.rs để tạo user_id
    - walletlogic::handler định nghĩa trait WalletManagerHandler với 12 phương thức quản lý ví
    - walletlogic::handler implement các chức năng như export, remove, update wallet, ký/gửi giao dịch
    - walletlogic::utils cung cấp hàm tạo user_id cho từng loại người dùng (Free, Premium, VIP)
    - walletlogic::crypto sử dụng AES-256-GCM và PBKDF2 với 100,000 vòng lặp cho bảo mật cao
    - walletmanager::api là lớp giao tiếp chính, gọi vào walletlogic::init và walletlogic::handler để thực thi logic ví
    - walletmanager::api sử dụng walletmanager::chain để quản lý kết nối blockchain cần thiết cho các thao tác như gửi giao dịch, lấy số dư
    - walletmanager::chain định nghĩa trait ChainManager và DefaultChainManager, có thể tương tác với defi::blockchain để lấy provider
    - walletmanager::types định nghĩa các cấu trúc dữ liệu công khai cho walletmanager
    - walletmanager::lib re-export các thành phần quan trọng cho các module khác (ví dụ: snipebot)
    - defi có thể dùng walletlogic::handler để truy xuất ví
    - defi::api cung cấp DefiApi để truy cập chức năng DeFi từ bên ngoài
    - defi::farm định nghĩa FarmingManager, sử dụng contracts.rs và blockchain.rs
    - defi::stake định nghĩa StakingManager, sử dụng contracts.rs và blockchain.rs
    - defi::blockchain định nghĩa trait BlockchainProvider, được implement bởi các provider trong blockchain/non_evm/ và các EVM provider (có thể nằm ngoài defi)
    - defi::contracts cung cấp các hàm tương tác với smart contracts cụ thể cho DeFi
    - defi::chain định nghĩa ChainId, được sử dụng bởi blockchain.rs và các provider
    - defi::security cung cấp các hàm kiểm tra an toàn, được gọi bởi farm.rs và stake.rs
    - defi::error định nghĩa DefiError, được sử dụng trong toàn bộ module defi
    - defi::constants chứa các hằng số cấu hình cho DeFi
    - defi::tests.rs và defi::tests/integration.rs kiểm thử các chức năng của module DeFi
    - config cung cấp WalletSystemConfig với default_chain_id, max_wallets và phương thức can_add_wallet
    - error định nghĩa WalletError sử dụng thiserror với 11 loại lỗi khác nhau và implement Clone
    - users::mod.rs kết nối các loại người dùng (free, premium, vip) và module subscription
    - users::free_user::manager quản lý người dùng miễn phí và giới hạn của họ, sử dụng cache.rs
    - users::premium_user::premium_user.rs quản lý người dùng premium, tương tác với subscription::manager
    - users::vip_user::vip_user.rs quản lý người dùng VIP, tương tác với subscription::manager, nft.rs, staking.rs
    - users::subscription::manager là trung tâm quản lý đăng ký, nâng/hạ cấp, tương tác với nhiều module khác (payment, staking, nft, auto_trade, walletmanager::api, cache.rs)
    - users::subscription::auto_trade quản lý thời gian auto-trade, sử dụng cache.rs
    - users::subscription::staking quản lý việc stake DMD token (ERC-1155) cho gói VIP
    - users::subscription::nft kiểm tra sở hữu NFT cho gói VIP
    - users::subscription::payment xử lý thanh toán qua blockchain
    - users::subscription::events phát sự kiện về thay đổi trạng thái đăng ký
    - users có thể liên kết với walletlogic qua user_id (được tạo trong walletlogic::utils)
    - tests/mod.rs khai báo các test modules cho free_user (auth_tests, limits_tests, records_tests)
    - tests/integration.rs kiểm tra toàn bộ vòng đời của ví: tạo, import/export, sign/balance
    - tests/auth_tests.rs kiểm tra chức năng xác thực người dùng với test_verify_free_user
    - tests/auth_tests.rs sử dụng MockWalletHandler từ free_user::test_utils
    - tests tuân thủ quy tắc "Viết integration test cho module" từ development_workflow.testing
    - main.rs dùng walletmanager và config để demo
*/

// Module structure của dự án wallet
pub mod error;         // Định nghĩa WalletError và các utility function
pub mod config;        // Cấu hình chung cho ví
pub mod walletlogic;   // Core logic cho ví blockchain
pub mod walletmanager; // API/UI layer để tương tác với ví
pub mod defi;          // DeFi functionality (farming, staking, blockchain interaction)
pub mod users;         // Quản lý người dùng và đăng ký
pub mod contracts;     // Module tương tác với smart contracts

/**
 * Hướng dẫn import:
 * 
 * 1. Import từ internal crates:
 * - use crate::walletlogic::handler::WalletHandler;
 * - use crate::walletmanager::api::WalletManagerApi;
 * - use crate::walletmanager::types::{WalletConfig, WalletInfo, SeedLength, WalletSecret};
 * - use crate::walletmanager::chain::{ChainConfig, ChainType, ChainManager, DefaultChainManager};
 * - use crate::users::subscription::manager::SubscriptionManager;
 * - use crate::users::subscription::staking::{StakingManager, TokenStake, StakeStatus};
 * 
 * 2. Import từ external crates (từ snipebot hoặc blockchain):
 * - use wallet::walletmanager::api::WalletManagerApi;
 * - use wallet::walletmanager::types::{WalletConfig, SeedLength};
 * - use wallet::walletmanager::chain::{ChainConfig, ChainType};
 * - use wallet::users::subscription::manager::SubscriptionManager;
 * - use wallet::users::subscription::staking::StakingManager;
 * 
 * 3. Import error types:
 * - use crate::error::{WalletError, Result};
 * - use crate::users::subscription::staking::StakingError;
 * 
 * 4. Import các events:
 * - use crate::users::subscription::events::{SubscriptionEvent, EventType};
 * 
 * 5. Pricing constants cho subscription:
 * - use crate::users::subscription::constants::{FREE_TO_PREMIUM_UPGRADE_PRICE_USDC, 
 *                                            PREMIUM_TO_VIP_UPGRADE_PRICE_USDC,
 *                                            FREE_TO_VIP_UPGRADE_PRICE_USDC,
 *                                            VIP_TWELVE_MONTH_PRICE_USDC,
 *                                            MIN_DMD_STAKE_AMOUNT,
 *                                            STAKED_DMD_APY_PERCENTAGE};
 */

//! Module quản lý ví và tài khoản người dùng.
//! 
//! Module này cung cấp các chức năng:
//! - Quản lý ví và tài khoản người dùng
//! - Xử lý giao dịch và thanh toán
//! - Quản lý subscription và VIP
//! - Tích hợp DeFi (staking và farming)
//! 
//! # Flow giữa các module
//! 
//! ```text
//! snipebot -> wallet -> blockchain
//!     ↓         ↓         ↓
//!     └─────────┴─────────┘
//! ```
//! ## Module users
//! 
//! Quản lý thông tin và trạng thái người dùng:
//! - `free_user`: Người dùng miễn phí
//! - `premium_user`: Người dùng premium
//! - `vip_user`: Người dùng VIP
//! 
//! ## Module defi
//! 
//! Quản lý các chức năng DeFi:
//! - `farm.rs`: Quản lý liquidity pools và farming
//! - `stake.rs`: Quản lý staking pools
//! - `constants.rs`: Các hằng số
//! - `error.rs`: Các loại lỗi
//! 
//! ## Module subscription
//! 
//! Quản lý subscription và thanh toán:
//! - `manager.rs`: Quản lý subscription
//! - `payment.rs`: Xử lý thanh toán
//! - `staking.rs`: Quản lý staking

//! 
//! # Các tính năng mới
//! 
//! 1. Module DeFi với staking và farming
//! 2. Cache layer tối ưu hóa
//! 3. Error handling cải tiến
//! 4. Thread safety trong async context
//! 5. Documentation đầy đủ
//! 6. Unit tests cơ bản
//! 
//! # Các tính năng dự kiến
//! 
//! 1. Integration tests
//! 2. Module tests
//! 3. Metrics chi tiết
//! 4. Logging nâng cao
//! 5. Documentation nâng cao
//! 6. Error handling nâng cao
//! 
//! # Các thay đổi gần đây
//! 
//! 1. Thêm module DeFi
//! 2. Tách constants và error types
//! 3. Cải thiện thread safety
//! 4. Cải thiện error handling
//! 5. Cải thiện documentation
//! 6. Thêm unit tests
//! 7. Tối ưu hóa cache layer
//! 8. Cải thiện logging
//! 9. Cải thiện metrics
//! 10. Cải thiện flow documentation
//! 11. Thêm Diamond blockchain provider với hỗ trợ các token chuẩn DRC-20, DRC-721, DRC-1155
//! 12. Thêm module `contracts` để tương tác với smart contracts (ban đầu là ERC-20)
//! 13. Cập nhật cấu trúc chi tiết của module `defi` trong manifest
//! 14. Cập nhật cấu trúc chi tiết của module `users` trong manifest
//! 15. Cập nhật cấu trúc chi tiết của module `walletmanager` trong manifest
//! 16. Thêm thông tin về các file kiểm thử trong thư mục `tests` (auth_tests.rs, limits_tests.rs, records_tests.rs, cache_tests.rs)
//! 17. Cập nhật mô tả cho file `cache.rs` với chi tiết về các loại cache và thao tác quản lý
//! 18. Bổ sung mô tả chi tiết cho file `improvement_report.md` về các vấn đề tiềm ẩn và hướng cải tiến
//! 19. Cải tiến DiamondProvider trong `defi/blockchain/non_evm/diamond.rs` với hỗ trợ tất cả các chức năng mới nhất của mạng Diamond, bao gồm transaction history và smart contract interaction
//! 20. Bổ sung thêm unit tests và integration tests cho DiamondProvider để đảm bảo độ tin cậy cao
//! 21. Tối ưu hóa hiệu suất của DiamondProvider thông qua cải tiến caching và batch request
//! 22. Cập nhật tài liệu API và hướng dẫn sử dụng cho DiamondProvider