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
    ├── cache.rs                   -> Hệ thống cache LRU với TTL, quản lý cache cho toàn bộ domain, user VIP và Premium, xử lý cache thread-safe
    ├── manifest.rs                -> Tài liệu tham chiếu module path [liên quan: tất cả các module, BẮT BUỘC đọc đầu tiên]
    ├── src/walletlogic/           -> Logic cốt lõi ví với xử lý lỗi chi tiết và logging
    │   ├── mod.rs                 -> Khai báo submodule [liên quan: tất cả các file trong walletlogic]
    │   ├── init.rs                -> Khởi tạo WalletManager, tạo/nhập ví với mã hóa seed/key [liên quan: handler.rs, utils.rs]
    │   ├── handler.rs             -> Xử lý ví, định nghĩa trait WalletManagerHandler với các phương thức quản lý ví [liên quan: utils.rs, walletmanager::api]
    │   ├── utils.rs               -> Các hàm tiện ích, định nghĩa UserType (Free, Premium, VIP) [liên quan: init.rs, handler.rs]
    ├── src/walletmanager/         -> API/UI cho ví, quản lý kết nối blockchain với xử lý lỗi chi tiết
    │   ├── mod.rs                 -> Khai báo các submodules (api, chain, types, lib) [liên quan: tất cả file trong walletmanager]
    │   ├── api.rs                 -> WalletManagerApi: API công khai để tạo, quản lý, tương tác với ví [liên quan: walletlogic::init, walletlogic::handler, types.rs, chain.rs, lib.rs]
    │   ├── chain.rs               -> ChainManager trait và implementations với cơ chế retry [liên quan: api.rs, walletlogic::handler, defi::blockchain]
    │   ├── types.rs               -> Các kiểu dữ liệu dùng trong walletmanager với validation [liên quan: api.rs, chain.rs, lib.rs]
    │   └── lib.rs                 -> Re-export các thành phần chính [liên quan: api.rs, types.rs]
    ├── src/defi/                  -> Chức năng DeFi (farming, staking, blockchain interaction)
    │   ├── mod.rs                 -> Khai báo và re-export các submodules và thành phần chính của DeFi [liên quan: api.rs, blockchain.rs, contracts.rs, security.rs, error.rs, constants.rs]
    │   ├── api.rs                 -> API công khai DefiApi cho DeFi (get_farming_opportunities, get_staking_opportunities) [liên quan: mod.rs, lib.rs]
    │   ├── lib.rs                 -> Re-export các thành phần chính từ module defi (DefiApi, DefiError, ...) [liên quan: api.rs, error.rs]
    │   ├── blockchain.rs          -> Định nghĩa interface BlockchainProvider, BlockchainConfig, BlockchainType và các hàm factory để tạo provider. Bổ sung đồng bộ hóa cache an toàn và quản lý cache chung cho các blockchain providers. Hỗ trợ kiểm tra và xác thực các chain ID trước khi sử dụng. Thêm phương thức đồng bộ cache như clear_provider_cache, clear_all_provider_caches và cleanup_expired_caches để đảm bảo nhất quán dữ liệu khi làm việc với nhiều providers. [liên quan: chain.rs, error.rs, constants.rs, blockchain/]
    │   ├── contracts.rs           -> Quản lý các smart contracts (ContractRegistry, ContractInterface, ContractManager) với cơ chế đồng bộ hóa và xử lý lỗi nâng cao [liên quan: blockchain.rs, erc20.rs, erc721.rs, erc1155.rs]
    │   ├── erc20.rs               -> Triển khai ERC20 token standard (Erc20Contract, TokenInfo, Erc20ContractBuilder) với cơ chế retry và xử lý lỗi chi tiết. Cải tiến decode_logs để xử lý lỗi an toàn thay vì gây panic. [liên quan: contracts.rs, blockchain.rs]
    │   ├── erc721.rs              -> Triển khai ERC721 NFT standard (Erc721Contract, NftInfo, Erc721ContractBuilder) với xử lý lỗi nâng cao. Cải tiến transfer_from và safe_transfer_from với kiểm tra địa chỉ zero, ownership và phê duyệt trước khi chuyển token. [liên quan: contracts.rs, blockchain.rs]
    │   ├── erc1155.rs             -> Triển khai ERC1155 Multi-Token standard (Erc1155Contract, Erc1155ContractBuilder) với xử lý lỗi nâng cao. Cải tiến decode_transfer_batch_event và các hàm chuyển token với các kiểm tra an toàn và logging đầy đủ. [liên quan: contracts.rs, blockchain.rs]
    │   ├── chain.rs               -> Định nghĩa ChainId và các thông tin liên quan đến chain (RPC URLs, explorer URLs) [liên quan: blockchain.rs]
    │   ├── security.rs            -> Các biện pháp bảo mật cho DeFi (validation, rate limiting, risk assessment) [liên quan: contracts.rs]
    │   ├── provider.rs            -> Quản lý các provider cho DeFi [liên quan: blockchain.rs, chain.rs]
    │   ├── error.rs               -> Định nghĩa DefiError cho module DeFi [liên quan: tất cả các file trong defi]
    │   ├── constants.rs           -> Các hằng số sử dụng trong module DeFi (timeouts, retries, default values) [liên quan: blockchain.rs]
    │   ├── crypto.rs              -> Mã hóa/giải mã dữ liệu sử dụng AES-256-GCM và PBKDF2, được di chuyển từ walletlogic [liên quan: contracts.rs, token/]
    │   ├── tests.rs               -> Unit tests cho các chức năng DeFi, kiểm tra các provider và tích hợp blockchain [liên quan: blockchain.rs, contracts.rs, erc20.rs, erc721.rs, erc1155.rs]
    │   ├── token/                 -> Các thành phần quản lý token
    │   │   └── manager.rs         -> TokenManager: Quản lý thông tin token với cache và validation [liên quan: erc20.rs, blockchain.rs, error.rs]
    │   ├── blockchain/            -> Implementations cho các blockchain providers
    │   │   ├── non_evm/           -> Providers cho các blockchain không tương thích EVM
    │   │   │   ├── solana.rs      -> Solana provider [liên quan: blockchain.rs]
    │   │   │   ├── tron.rs        -> Tron provider với cơ chế caching tối ưu và đồng bộ hóa, bao gồm auto-cleanup cho expired cache, sử dụng RwLock để đảm bảo thread-safety khi truy cập cache. Cải thiện hiệu suất gọi API với cache cho các phương thức call_full_node_api, call_solidity_node_api và call_event_server_api. [liên quan: blockchain.rs]
    │   │   │   ├── hedera.rs      -> Hedera provider [liên quan: blockchain.rs]
    │   │   │   ├── cosmos.rs      -> Cosmos provider [liên quan: blockchain.rs]
    │   │   │   ├── near.rs        -> NEAR provider [liên quan: blockchain.rs]
    │   │   │   └── diamond.rs     -> Diamond provider với cơ chế checksum đầy đủ, retry, cache và xử lý lỗi nâng cao. Bổ sung các phương thức quản lý cache như clear_cache_item, clear_all_cache, và cleanup_cache để đồng bộ với cơ chế cache được cập nhật trong blockchain.rs. [liên quan: blockchain.rs]
    ├── src/users/                 -> Quản lý người dùng (free, premium, VIP) và đăng ký
    │   ├── mod.rs                 -> Khai báo và re-export các submodules (free_user, subscription, premium_user, vip_user) [liên quan: tất cả file và thư mục trong users]
    │   ├── free_user/             -> Logic cho người dùng miễn phí
    │   │   ├── mod.rs             -> Khai báo các modules con (types, manager, auth, limits, records, queries, test_utils) [liên quan: tất cả file trong free_user]
    │   │   ├── types.rs           -> Định nghĩa kiểu dữ liệu FreeUserData, UserStatus, TransactionType [liên quan: manager.rs, auth.rs, limits.rs]
    │   │   ├── manager.rs         -> Quản lý FreeUserManager, sử dụng cache và triển khai cơ chế backup tự động để đảm bảo an toàn dữ liệu khi có lỗi [liên quan: types.rs, auth.rs, limits.rs, records.rs, queries.rs, cache.rs]
    │   │   ├── auth.rs            -> Xác thực người dùng miễn phí (login, register, verify) [liên quan: types.rs, manager.rs]
    │   │   ├── limits.rs          -> Kiểm tra giới hạn giao dịch [liên quan: types.rs, manager.rs]
    │   │   ├── records.rs         -> Ghi nhận hoạt động [liên quan: types.rs, manager.rs]
    │   │   ├── queries.rs         -> Truy vấn dữ liệu người dùng qua cache [liên quan: types.rs, manager.rs, cache.rs]
    │   │   └── test_utils.rs      -> Công cụ kiểm thử với MockWalletHandler [liên quan: tests/auth_tests.rs]
    │   ├── subscription/          -> Logic quản lý đăng ký, thanh toán, staking, NFT, auto-trade
    │   │   ├── mod.rs             -> Khai báo và re-export các submodules và types chính [liên quan: tất cả file trong subscription]
    │   │   ├── manager.rs         -> SubscriptionManager: điều phối chính, xử lý nâng/hạ cấp, xác minh với cơ chế đồng bộ hóa an toàn và xử lý lỗi chi tiết [liên quan: user_subscription.rs, types.rs, auto_trade.rs, nft.rs, staking.rs, payment.rs, events.rs, walletmanager::api, vip.rs]
    │   │   ├── user_subscription.rs -> Định nghĩa UserSubscription, SubscriptionConverter với validation đầy đủ [liên quan: manager.rs, types.rs, auto_trade.rs, nft.rs]
    │   │   ├── types.rs           -> Định nghĩa các kiểu dữ liệu SubscriptionType, SubscriptionStatus, Feature, PaymentToken [liên quan: manager.rs, user_subscription.rs, staking.rs]
    │   │   ├── constants.rs       -> Hằng số (giá, stake amounts, APY) [liên quan: manager.rs, staking.rs, payment.rs]
    │   │   ├── auto_trade.rs      -> AutoTradeManager: quản lý thời gian auto-trade với cơ chế đồng bộ hóa an toàn, persistence và rate limiting [liên quan: manager.rs, user_subscription.rs, cache.rs]
    │   │   ├── nft.rs             -> Quản lý NFT cho VIP (VipNftInfo, NonNftVipStatus), kiểm tra sở hữu với cross-check và caching [liên quan: manager.rs, walletmanager::api, vip_user.rs]
    │   │   ├── staking.rs         -> Quản lý staking DMD token (ERC-1155), TokenStake, StakeStatus với xử lý lỗi nâng cao [liên quan: manager.rs, types.rs, constants.rs, walletmanager::api]
    │   │   ├── payment.rs         -> Xử lý thanh toán, xác minh giao dịch blockchain với retry mechanism [liên quan: manager.rs, constants.rs, walletmanager::api]
    │   │   ├── vip.rs             -> Logic đặc biệt cho VIP liên quan đến subscription [liên quan: manager.rs]
    │   │   ├── events.rs          -> Định nghĩa và phát sự kiện SubscriptionEvent, EventType, EventEmitter [liên quan: manager.rs, user_subscription.rs]
    │   │   ├── utils.rs           -> Các hàm tiện ích cho subscription [liên quan: tất cả file trong subscription]
    │   │   └── tests.rs           -> Unit tests cho các chức năng subscription [liên quan: tất cả file trong subscription]
    │   ├── premium_user.rs        -> Logic người dùng premium (PremiumUserData, PremiumUserManager) [liên quan: subscription::manager, walletmanager::api]
    │   └── vip_user.rs            -> Logic người dùng VIP (VipUserData, VipUserManager), quản lý NFT/staking với xử lý lỗi chi tiết [liên quan: subscription::manager, subscription::nft, subscription::staking, walletmanager::api]
    ├── tests/                     -> Integration tests cho wallet
    │   ├── mod.rs                 -> Khai báo các test modules cho free_user và các module khác [liên quan: auth_tests.rs, limits_tests.rs, records_tests.rs, cache_tests.rs, defi_tests.rs, integration.rs]
    │   ├── auth_tests.rs          -> Tests cho xác thực người dùng, test_verify_free_user [liên quan: users::free_user::auth, free_user::test_utils]
    │   ├── limits_tests.rs        -> Tests cho kiểm tra giới hạn giao dịch [liên quan: users::free_user::limits]
    │   ├── records_tests.rs       -> Tests cho ghi nhận hoạt động [liên quan: users::free_user::records]
    │   ├── cache_tests.rs         -> Kiểm thử hệ thống cache
    │   ├── defi_tests.rs          -> Integration tests cho module DeFi [liên quan: defi::*, bao gồm tests cho farm, stake, blockchain, cache, logging, metrics]
    │   └── integration.rs         -> Integration tests cho wallet lifecycle, import/export, sign/balance [liên quan: walletmanager::api, walletlogic, config]

    Mối liên kết:
    - walletlogic phụ thuộc error (dùng WalletError)
    - walletlogic::init quản lý WalletManager với wallets: Arc<RwLock<HashMap<Address, WalletInfo>>>
    - walletlogic::init thực hiện tạo và nhập ví, sử dụng utils.rs để tạo user_id
    - walletlogic::handler định nghĩa trait WalletManagerHandler với 12 phương thức quản lý ví
    - walletlogic::handler implement các chức năng như export, remove, update wallet, ký/gửi giao dịch
    - walletlogic::utils cung cấp hàm tạo user_id cho từng loại người dùng (Free, Premium, VIP)
    - walletmanager::api là lớp giao tiếp chính, gọi vào walletlogic::init và walletlogic::handler
    - walletmanager::api sử dụng walletmanager::chain để quản lý kết nối blockchain với cơ chế retry
    - walletmanager::chain định nghĩa trait ChainManager và DefaultChainManager, có thể tương tác với defi::blockchain
    - walletmanager::types định nghĩa các cấu trúc dữ liệu công khai cho walletmanager với validation
    - walletmanager::lib re-export các thành phần quan trọng cho các module khác
    - defi có thể dùng walletlogic::handler để truy xuất ví
    - defi::api cung cấp DefiApi để truy cập chức năng DeFi từ bên ngoài
    - defi::crypto sử dụng AES-256-GCM và PBKDF2 với 100,000 vòng lặp cho mã hóa/giải mã dữ liệu nhạy cảm
    - defi::blockchain định nghĩa trait BlockchainProvider, được implement bởi các provider trong blockchain/non_evm/
    - defi::blockchain cung cấp BlockchainProviderFactory với các phương thức để tạo, quản lý và xóa cache cho các provider
    - defi::contracts cung cấp các hàm tương tác với smart contracts cụ thể cho DeFi, với cơ chế đồng bộ hóa và xử lý lỗi nâng cao
    - defi::chain định nghĩa ChainId, được sử dụng bởi blockchain.rs và các provider
    - defi::provider quản lý và khởi tạo các blockchain provider
    - defi::security cung cấp các hàm kiểm tra an toàn, được gọi bởi contracts.rs
    - defi::error định nghĩa DefiError, được sử dụng trong toàn bộ module defi
    - defi::constants chứa các hằng số cấu hình cho DeFi
    - defi::token::manager quản lý thông tin token với cache và hỗ trợ validation chi tiết
    - defi::blockchain/non_evm/ chứa các provider cho blockchain không tương thích EVM
    - defi::blockchain/non_evm/diamond.rs cung cấp Diamond blockchain provider với hỗ trợ các token chuẩn DRC-20, DRC-721, DRC-1155, kèm theo cơ chế checksum đầy đủ, retry, cache và xử lý lỗi nâng cao
    - defi::blockchain/non_evm/tron.rs cung cấp Tron blockchain provider với cơ chế caching tối ưu và đồng bộ hóa, tự động dọn dẹp cache hết hạn
    - defi::blockchain/non_evm/solana.rs, hedera.rs, cosmos.rs, near.rs cung cấp các provider cho các blockchain tương ứng
    - config cung cấp WalletSystemConfig với default_chain_id, max_wallets và phương thức can_add_wallet
    - error định nghĩa WalletError sử dụng thiserror với 11 loại lỗi khác nhau và implement Clone
    - cache.rs cung cấp hệ thống cache thread-safe với TTL dùng LRU cache
    - users::mod.rs kết nối các loại người dùng (free, premium, vip) và module subscription với cấu trúc module cải thiện để tôn trọng encapsulation
    - users::free_user::manager quản lý người dùng miễn phí và giới hạn của họ, sử dụng cache.rs, và cung cấp cơ chế backup cho dữ liệu người dùng
    - users::premium_user quản lý người dùng premium, tương tác với subscription::manager
    - users::vip_user quản lý người dùng VIP, tương tác với subscription::manager, nft.rs, staking.rs
    - users::subscription::manager là trung tâm quản lý đăng ký, nâng/hạ cấp, với cơ chế đồng bộ hóa an toàn và xử lý lỗi chi tiết, tương tác với nhiều module khác (payment, staking, nft, auto_trade, walletmanager::api, cache.rs)
    - users::subscription::auto_trade quản lý thời gian auto-trade với cơ chế đồng bộ hóa an toàn, persistence và rate limiting, sử dụng cache.rs
    - users::subscription::staking quản lý việc stake DMD token (ERC-1155) cho gói VIP với xử lý lỗi nâng cao
    - users::subscription::nft kiểm tra sở hữu NFT cho gói VIP với cross-check và caching
    - users::subscription::payment xử lý thanh toán qua blockchain với retry mechanism
    - users::subscription::events phát sự kiện về thay đổi trạng thái đăng ký
    - users có thể liên kết với walletlogic qua user_id (được tạo trong walletlogic::utils)
    - tests/mod.rs khai báo các test modules cho free_user và các module khác (auth_tests, limits_tests, records_tests, cache_tests, defi_tests, integration)
    - tests/auth_tests.rs kiểm tra chức năng xác thực người dùng với test_verify_free_user
    - tests/auth_tests.rs sử dụng MockWalletHandler từ free_user::test_utils
    - tests/defi_tests.rs chứa các integration tests cho module DeFi, bao gồm tests cho farm, stake, blockchain, cache, logging, metrics
    - tests tuân thủ quy tắc "Viết integration test cho module" từ development_workflow.testing
    - main.rs dùng walletmanager và config để demo
*/

// Module structure của dự án wallet
pub mod error;         // Định nghĩa WalletError và các utility function
pub mod config;        // Cấu hình chung cho ví
pub mod walletlogic;   // Core logic cho ví blockchain
pub mod walletmanager; // API/UI layer để tương tác với ví
pub mod defi;          // DeFi functionality (farming, staking, blockchain interaction, contracts)
pub mod users;         // Quản lý người dùng và đăng ký
pub mod cache;         // Hệ thống cache LRU với TTL cho dữ liệu tái sử dụng

/**
 * Hướng dẫn import:
 * 
 * 1. Import từ internal crates:
 * - use crate::walletlogic::handler::WalletHandler;
 * - use crate::walletlogic::init::{create_wallet_internal, import_wallet_internal};
 * - use crate::walletlogic::utils::{generate_user_id, is_seed_phrase};
 * - use crate::walletmanager::api::WalletManagerApi;
 * - use crate::walletmanager::types::{WalletConfig, WalletInfo, SeedLength, WalletSecret};
 * - use crate::walletmanager::chain::{ChainConfig, ChainType, ChainManager, DefaultChainManager};
 * - use crate::users::subscription::manager::SubscriptionManager;
 * - use crate::users::subscription::staking::{StakingManager, TokenStake, StakeStatus};
 * - use crate::defi::api::DefiApi;
 * - use crate::defi::blockchain::non_evm::diamond::DiamondBlockchainProvider;
 * - use crate::defi::provider::{get_provider, ProviderConfig};
 * - use crate::defi::erc20::{Erc20Contract, Erc20ContractBuilder};
 * - use crate::defi::contracts::{ContractInterface, ContractRegistry, ContractFactory};
 * - use crate::defi::crypto::{encrypt_data, decrypt_data};
 * - use crate::defi::token::manager::TokenManager;
 * - use crate::cache::{CacheSystem, CacheConfig, CacheKey};
 * 
 * 2. Import từ external crates (từ snipebot hoặc blockchain):
 * - use wallet::walletmanager::api::WalletManagerApi;
 * - use wallet::walletmanager::types::{WalletConfig, SeedLength};
 * - use wallet::walletmanager::chain::{ChainConfig, ChainType};
 * - use wallet::users::managers::SubscriptionManager;
 * - use wallet::users::types::{UserSubscription, VipUserData};
 * - use wallet::users::FreeUserManager;
 * - use wallet::users::PremiumUserManager;
 * - use wallet::users::VipUserManager;
 * - use wallet::defi::blockchain::non_evm::diamond::DiamondBlockchainProvider;
 * - use wallet::defi::erc20::Erc20Contract;
 * - use wallet::defi::erc721::Erc721Contract;
 * - use wallet::defi::contracts::ContractRegistry;
 * - use wallet::defi::token::manager::TokenManager;
 * 
 * 3. Import error types:
 * - use crate::error::{WalletError, Result};
 * - use crate::users::subscription::staking::StakingError;
 * - use crate::defi::error::DefiError;
 * - use crate::defi::contracts::ContractError;
 * 
 * 4. Import các events:
 * - use crate::users::subscription::events::{SubscriptionEvent, EventType, EventEmitter};
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
//! - `blockchain.rs`: Interface BlockchainProvider
//! - `provider.rs`: Quản lý các provider
//! - `contracts.rs`: Quản lý các smart contracts và interface
//! - `erc20.rs`: Tương tác với ERC-20 contracts
//! - `erc721.rs`: Tương tác với ERC-721 NFT contracts
//! - `erc1155.rs`: Tương tác với ERC-1155 Multi-Token contracts
//! - `blockchain/non_evm/`: Các provider cho blockchain không tương thích EVM
//! - `token/manager.rs`: Quản lý thông tin token với cơ chế cache và validation
//! - `constants.rs`: Các hằng số
//! - `error.rs`: Các loại lỗi
//! 
//! ## Module subscription
//! 
//! Quản lý subscription và thanh toán:
//! - `manager.rs`: Quản lý subscription
//! - `payment.rs`: Xử lý thanh toán
//! - `staking.rs`: Quản lý staking
//! - `auto_trade.rs`: Quản lý auto-trade
//! - `nft.rs`: Kiểm tra sở hữu NFT
//! 
//! ## Module cache
//!
//! Hệ thống cache thread-safe với TTL:
//! - Hỗ trợ LRU cache
//! - Quản lý TTL cho các loại dữ liệu khác nhau
//! - Xử lý thread safety với RwLock
//! 
//! # Các tính năng chính
//! 
//! 1. Quản lý ví an toàn với mã hóa AES-256-GCM
//! 2. Hỗ trợ đa blockchain (EVM và non-EVM)
//! 3. Quản lý người dùng và subscription
//! 4. Tích hợp DeFi (staking, farming)
//! 5. Cache thread-safe với TTL
//! 6. Xử lý lỗi chi tiết và logging
//! 7. Unit tests và integration tests
//! 
//! # Các cải tiến gần đây
//! 
//! 1. Module stake:
//!    - Triển khai đầy đủ stake pools
//!    - Thêm cơ chế cache và retry
//!    - Cải thiện tính toán APY và rewards
//!    - Hỗ trợ multi-chain staking
//!    - Thêm persistence storage
//!    - Bổ sung unit tests và integration tests
//! 
//! 2. Module defi:
//!    - Cải thiện xử lý lỗi blockchain
//!    - Thêm retry mechanism cho API calls
//!    - Tối ưu hóa cache và performance
//!    - Bổ sung validation và security checks
//!    - Thêm TokenManager để quản lý thông tin token với cache hiệu quả
//! 
//! 3. Module users:
//!    - Cải thiện quản lý subscription
//!    - Thêm validation cho thời gian đăng ký
//!    - Tối ưu hóa cache cho user data
//!    - Bổ sung rate limiting cho auto-trade
//!    - Cải thiện encapsulation trong tổ chức module
//!    - Thêm cơ chế backup tự động cho free_user
//! 
//! 4. Module walletlogic:
//!    - Hỗ trợ đầy đủ cho non-EVM blockchain
//!    - Cải thiện xử lý lỗi và logging
//!    - Tối ưu hóa performance
//!    - Bổ sung security checks
//! 
//! 5. Cấu trúc test:
//!    - Hợp nhất các test từ `wallet/src/defi/tests` vào `wallet/tests`
//!    - Tạo file `defi_tests.rs` mới trong `wallet/tests`
//!    - Cập nhật `mod.rs` để include module `defi_tests`
//!    - Thêm tests.rs trong module defi cho unit tests

// Các cập nhật quan trọng:
/**
 * 05-05-2023: Khởi tạo cấu trúc module wallet
 * 08-05-2023: Thêm module walletlogic với init.rs và handler.rs
 * 10-05-2023: Thêm module walletmanager với api.rs và chain.rs
 * 12-05-2023: Thêm error.rs và config.rs
 * 15-05-2023: Thêm mã hóa AES-256-GCM trong crypto.rs
 * 18-05-2023: Thêm module users với free_user
 * 20-05-2023: Thêm subscription manager và payment
 * 22-05-2023: Thêm hỗ trợ premium_user và vip_user
 * 25-05-2023: Thêm module defi với blockchain.rs và chain.rs
 * 28-05-2023: Thêm các provider cho non_evm blockchains
 * 01-06-2023: Thêm module contracts với erc20.rs
 * 03-06-2023: Thêm cache.rs với LRU cache
 * 05-06-2023: Thêm subscription staking và nft
 * 08-06-2023: Thêm events.rs cho subscription events
 * 10-06-2023: Thêm provider.rs trong defi
 * 12-06-2023: Thêm security.rs cho DeFi
 * 15-06-2023: Tối ưu hóa cache với thread safety
 * 18-06-2023: Cập nhật manifest.rs để phản ánh cấu trúc thực tế của dự án
 * 20-06-2023: Di chuyển crypto.rs từ walletlogic sang defi để tái sử dụng cho mã hóa/giải mã dữ liệu DeFi
 * 22-06-2023: Thêm TokenManager trong token/manager.rs để quản lý thông tin token
 * 25-06-2023: Cải thiện cơ chế backup trong free_user/manager.rs
 * 28-06-2023: Cải thiện tổ chức module users để tôn trọng encapsulation
 * 30-06-2023: Thêm unit tests riêng cho defi và các module con
 */

/// # Danh Sách Trait Quan Trọng
/// 
/// Các trait chính được định nghĩa trong domain wallet, có thể được sử dụng bởi các domain khác.
/// 
/// ## Core Traits
/// 
/// ```
/// // WalletManagerHandler - quản lý các thao tác với ví 
/// pub trait WalletManagerHandler: Send + Sync + 'static {
///     async fn create_wallet(&self, config: WalletConfig) -> Result<WalletInfo>;
///     async fn import_wallet(&self, seed_phrase: String, password: String) -> Result<WalletInfo>;
///     async fn export_wallet(&self, address: Address, password: String) -> Result<String>;
///     async fn list_wallets(&self) -> Result<Vec<WalletInfo>>;
///     async fn update_wallet(&self, address: Address, name: Option<String>) -> Result<WalletInfo>;
///     async fn get_balance(&self, address: Address, chain_id: u64) -> Result<U256>;
///     async fn sign_transaction(&self, address: Address, tx: TransactionRequest) -> Result<Signature>;
///     async fn send_transaction(&self, address: Address, tx: TransactionRequest) -> Result<TxHash>;
///     async fn estimate_gas(&self, tx: TransactionRequest, chain_id: u64) -> Result<U256>;
///     async fn verify_signature(&self, address: Address, message: String, signature: Signature) -> Result<bool>;
///     async fn sign_message(&self, address: Address, message: String) -> Result<Signature>;
///     async fn remove_wallet(&self, address: Address) -> Result<()>;
/// }
/// 
/// // ChainManager - quản lý kết nối blockchain 
/// pub trait ChainManager: Send + Sync + 'static {
///     async fn get_provider(&self, chain_id: u64) -> Result<Arc<Provider<Http>>>;
///     async fn get_balance(&self, address: Address, chain_id: u64) -> Result<U256>;
///     async fn get_transaction(&self, tx_hash: TxHash, chain_id: u64) -> Result<Option<Transaction>>;
///     async fn get_transaction_receipt(&self, tx_hash: TxHash, chain_id: u64) -> Result<Option<TransactionReceipt>>;
///     async fn send_transaction(&self, signed_tx: SignedTransaction, chain_id: u64) -> Result<TxHash>;
///     async fn estimate_gas(&self, tx: TransactionRequest, chain_id: u64) -> Result<U256>;
/// }
/// 
/// // BlockchainProvider - giao diện chung cho tất cả providers 
/// pub trait BlockchainProvider: Send + Sync {
///     fn chain_id(&self) -> ChainId;
///     fn blockchain_type(&self) -> BlockchainType;
///     async fn is_connected(&self) -> Result<bool, DefiError>;
///     async fn get_block_number(&self) -> Result<u64, DefiError>;
///     async fn get_balance(&self, address: &str) -> Result<U256, DefiError>;
/// }
/// 
/// // DefiProvider - giao diện cho DeFi operations 
/// pub trait DefiProvider: Send + Sync + 'static {
///     fn chain_id(&self) -> ChainId;
///     fn provider_type(&self) -> ProviderType;
///     async fn farm_manager(&self) -> &Box<dyn FarmManager>;
///     async fn stake_manager(&self) -> &Box<dyn StakeManager>;
///     async fn add_farm_pool(&self, config: FarmPoolConfig) -> Result<(), DefiError>;
///     async fn add_stake_pool(&self, config: StakePoolConfig) -> Result<(), DefiError>;
///     async fn sync_pools(&self) -> Result<(), DefiError>;
/// }
/// 
/// // ContractInterface - giao diện chung cho smart contracts 
/// pub trait ContractInterface: Send + Sync + 'static {
///     fn address(&self) -> Address;
///     fn chain_id(&self) -> ChainId;
///     fn contract_type(&self) -> ContractType;
///     fn is_verified(&self) -> bool;
///     async fn get_abi(&self) -> Result<String, ContractError>;
///     async fn get_bytecode(&self) -> Result<Bytes, ContractError>;
///     async fn call(&self, function: &str, params: Vec<Token>) -> Result<Vec<Token>, ContractError>;
/// }
/// 
/// // Cache trait - quản lý caching 
/// pub trait Cache<K, V> {
///     fn insert(&mut self, key: K, value: V) -> Option<V>;
///     fn get(&mut self, key: &K) -> Option<&V>;
///     fn remove(&mut self, key: &K) -> Option<V>;
///     fn contains_key(&self, key: &K) -> bool;
///     fn len(&self) -> usize;
///     fn is_empty(&self) -> bool;
///     fn clear(&mut self);
/// }
/// 
/// // SubscriptionConverter - chuyển đổi giữa các loại subscription 
/// pub trait SubscriptionConverter {
///     fn to_premium(&self) -> Result<UserSubscription, WalletError>;
///     fn to_vip(&self) -> Result<UserSubscription, WalletError>;
///     fn to_free(&self) -> Result<UserSubscription, WalletError>;
/// }
/// 
/// // EventListener - lắng nghe các sự kiện 
/// pub trait EventListener: Send + Sync {
///     fn on_event(&self, event: &Event);
/// }
/// 
/// // SubscriptionEventListener - lắng nghe các sự kiện đăng ký 
/// pub trait SubscriptionEventListener {
///     fn on_subscription_created(&self, subscription: &UserSubscription);
///     fn on_subscription_updated(&self, subscription: &UserSubscription);
///     fn on_subscription_expired(&self, subscription: &UserSubscription);
///     fn on_subscription_cancelled(&self, subscription: &UserSubscription);
/// }
/// ```
/// 
/// ## Blockchain Providers Implementations
/// 
/// Tất cả blockchain providers đều implement BlockchainProvider trait:
/// 
/// ```
/// // DiamondBlockchainProvider - Provider cho blockchain Diamond
/// pub struct DiamondProvider {
///     pub config: BlockchainConfig,
///     client: Arc<RwLock<DiamondClient>>,
///     cache: DiamondCache,
/// }
/// 
/// // TronBlockchainProvider - Provider cho blockchain Tron
/// pub struct TronProvider {
///     pub config: BlockchainConfig,
///     pub client: Arc<RwLock<TronClient>>,
///     cache: TronCache,
/// }
/// 
/// // SolanaBlockchainProvider - Provider cho blockchain Solana
/// pub struct SolanaProvider {
///     pub config: BlockchainConfig,
///     client: Arc<RwLock<SolanaClient>>,
///     cache: SolanaCache,
/// }
/// 
/// // Các provider khác: HederaProvider, CosmosProvider, NearProvider...
/// ```
/// 
/// ## Token Contract Interfaces
/// 
/// ```
/// // Erc20Interface - giao diện cho ERC20 tokens
/// pub trait Erc20Interface {
///     async fn total_supply(&self) -> Result<U256, ContractError>;
///     async fn balance_of(&self, account: Address) -> Result<U256, ContractError>;
///     async fn transfer(&self, recipient: Address, amount: U256) -> Result<bool, ContractError>;
///     async fn allowance(&self, owner: Address, spender: Address) -> Result<U256, ContractError>;
///     async fn approve(&self, spender: Address, amount: U256) -> Result<bool, ContractError>;
///     async fn transfer_from(&self, sender: Address, recipient: Address, amount: U256) -> Result<bool, ContractError>;
/// }
/// 
/// // Erc721Interface - giao diện cho ERC721 NFTs
/// pub trait Erc721Interface {
///     async fn balance_of(&self, owner: Address) -> Result<U256, ContractError>;
///     async fn owner_of(&self, token_id: U256) -> Result<Address, ContractError>;
///     async fn transfer_from(&self, from: Address, to: Address, token_id: U256) -> Result<(), ContractError>;
///     async fn safe_transfer_from(&self, from: Address, to: Address, token_id: U256) -> Result<(), ContractError>;
///     async fn approve(&self, to: Address, token_id: U256) -> Result<(), ContractError>;
///     async fn set_approval_for_all(&self, operator: Address, approved: bool) -> Result<(), ContractError>;
///     async fn get_approved(&self, token_id: U256) -> Result<Address, ContractError>;
///     async fn is_approved_for_all(&self, owner: Address, operator: Address) -> Result<bool, ContractError>;
/// }
/// 
/// // Erc1155Interface - giao diện cho ERC1155 Multi-Token
/// pub trait Erc1155Interface {
///     async fn balance_of(&self, account: Address, id: U256) -> Result<U256, ContractError>;
///     async fn balance_of_batch(&self, accounts: Vec<Address>, ids: Vec<U256>) -> Result<Vec<U256>, ContractError>;
///     async fn safe_transfer_from(&self, from: Address, to: Address, id: U256, amount: U256, data: Vec<u8>) -> Result<(), ContractError>;
///     async fn safe_batch_transfer_from(&self, from: Address, to: Address, ids: Vec<U256>, amounts: Vec<U256>, data: Vec<u8>) -> Result<(), ContractError>;
///     async fn set_approval_for_all(&self, operator: Address, approved: bool) -> Result<(), ContractError>;
///     async fn is_approved_for_all(&self, account: Address, operator: Address) -> Result<bool, ContractError>;
/// }
/// ```

/// # API Public và Import/Export Patterns
/// 
/// Các API và patterns import/export chính có thể được sử dụng bởi các domain khác.
/// 
/// ## Core APIs
/// 
/// ### WalletManagerApi
/// 
/// API chính để tương tác với wallet, được export và sử dụng bởi nhiều domains khác.
/// 
/// ```rust
/// // Tạo instance WalletManagerApi
/// let api = WalletManagerApi::new()?;
/// 
/// // Tạo ví mới
/// let wallet = api.create_wallet("My Wallet", "password123", SeedLength::Words12).await?;
/// 
/// // Import ví từ seed phrase
/// let wallet = api.import_wallet("seed phrase...", "password123", "Imported Wallet").await?;
/// 
/// // Lấy balance của ví
/// let balance = api.get_balance(wallet.address, 1).await?; // Chain ID = 1 (Ethereum)
/// 
/// // Ký và gửi transaction
/// let tx_request = TransactionRequest::new()
///     .to(recipient_address)
///     .value(amount)
///     .gas_price(gas_price);
/// let tx_hash = api.send_transaction(wallet.address, tx_request).await?;
/// ```
/// 
/// ### DefiApi
/// 
/// API để tương tác với các chức năng DeFi.
/// 
/// ```rust
/// // Tạo instance DefiApi
/// let defi_api = DefiApi::new(WalletManagerApi::new()?);
/// 
/// // Lấy danh sách cơ hội farming
/// let opportunities = defi_api.get_farming_opportunities(ChainId::EthereumMainnet).await?;
/// 
/// // Lấy danh sách cơ hội staking
/// let stake_pools = defi_api.get_staking_opportunities(ChainId::EthereumMainnet).await?;
/// 
/// // Thêm liquidity vào farm pool
/// defi_api.add_liquidity("user_id", pool_address, amount).await?;
/// 
/// // Stake token
/// defi_api.stake("user_id", pool_address, amount, lock_time).await?;
/// ```
/// 
/// ### SubscriptionManager
/// 
/// API quản lý đăng ký và subscription.
/// 
/// ```rust
/// // Tạo instance SubscriptionManager
/// let manager = SubscriptionManager::new(wallet_api);
/// 
/// // Tạo subscription mới
/// let subscription = manager.create_subscription(
///     "user_id",
///     SubscriptionType::Premium,
///     payment_address,
///     Some(payment_token)
/// ).await?;
/// 
/// // Nâng cấp subscription
/// let premium_sub = manager.upgrade_subscription(
///     "user_id", 
///     SubscriptionType::Premium,
///     payment_address,
///     Some(PaymentToken::USDC)
/// ).await?;
/// 
/// // Kiểm tra trạng thái subscription
/// let status = manager.check_subscription_status("user_id").await?;
/// ```
/// 
/// ## Import Patterns
/// 
/// ### 1. Import từ internal crates (trong cùng project):
/// 
/// ```rust
/// // Core modules
/// use crate::walletlogic::handler::WalletManagerHandler;
/// use crate::walletlogic::init::{create_wallet_internal, import_wallet_internal};
/// use crate::walletlogic::utils::{generate_user_id, is_seed_phrase};
/// 
/// // Wallet Manager
/// use crate::walletmanager::api::WalletManagerApi;
/// use crate::walletmanager::types::{WalletConfig, WalletInfo, SeedLength, WalletSecret};
/// use crate::walletmanager::chain::{ChainConfig, ChainType, ChainManager, DefaultChainManager};
/// 
/// // Subscription & Users
/// use crate::users::subscription::manager::SubscriptionManager;
/// use crate::users::subscription::staking::{StakingManager, TokenStake, StakeStatus};
/// use crate::users::subscription::types::{SubscriptionType, SubscriptionStatus};
/// use crate::users::subscription::payment::PaymentProcessor;
/// use crate::users::free_user::manager::FreeUserManager;
/// use crate::users::premium_user::PremiumUserManager;
/// use crate::users::vip_user::VipUserManager;
/// 
/// // DeFi
/// use crate::defi::api::DefiApi;
/// use crate::defi::blockchain::non_evm::diamond::DiamondBlockchainProvider;
/// use crate::defi::provider::{get_provider, ProviderConfig, DefiProvider};
/// use crate::defi::erc20::{Erc20Contract, Erc20ContractBuilder};
/// use crate::defi::contracts::{ContractInterface, ContractRegistry, ContractFactory};
/// use crate::defi::crypto::{encrypt_data, decrypt_data};
/// use crate::defi::token::manager::TokenManager;
/// 
/// // Cache & Error handling
/// use crate::cache::{CacheSystem, CacheConfig, CacheKey};
/// use crate::error::{WalletError, Result};
/// ```
/// 
/// ### 2. Import từ external crates (từ snipebot, blockchain hoặc domain khác):
/// 
/// ```rust
/// // Main API
/// use wallet::walletmanager::api::WalletManagerApi;
/// use wallet::walletmanager::types::{WalletConfig, SeedLength};
/// use wallet::walletmanager::chain::{ChainConfig, ChainType};
/// 
/// // Users & Subscription
/// use wallet::users::managers::SubscriptionManager;
/// use wallet::users::types::{UserSubscription, VipUserData};
/// use wallet::users::FreeUserManager;
/// use wallet::users::PremiumUserManager;
/// use wallet::users::VipUserManager;
/// 
/// // DeFi
/// use wallet::defi::blockchain::non_evm::diamond::DiamondBlockchainProvider;
/// use wallet::defi::erc20::Erc20Contract;
/// use wallet::defi::erc721::Erc721Contract;
/// use wallet::defi::contracts::ContractRegistry;
/// use wallet::defi::token::manager::TokenManager;
/// 
/// // Core & Error handling
/// use wallet::error::{WalletError, Result};
/// use wallet::cache::CacheSystem;
/// ```
/// 
/// ## Export Keys từ lib.rs
/// 
/// Đây là những items được re-export bởi lib.rs và có thể được import trực tiếp từ crate root:
/// 
/// ```rust
/// // Re-export trong lib.rs cho import thuận tiện
/// pub use crate::walletmanager::api::WalletManagerApi;
/// pub use crate::walletmanager::types::{WalletConfig, WalletInfo, SeedLength, WalletSecret};
/// pub use crate::walletmanager::chain::{ChainConfig, ChainType, ChainManager, DefaultChainManager};
/// pub use crate::error::{WalletError, Result};
/// pub use crate::config::WalletSystemConfig;
/// pub use crate::users::subscription::manager::SubscriptionManager;
/// pub use crate::users::subscription::staking::{StakingManager, TokenStake, StakeStatus};
/// pub use crate::defi::api::DefiApi;
/// pub use crate::defi::error::DefiError;
/// pub use crate::cache::{CacheSystem, CacheConfig, CacheKey};
/// pub use crate::contracts::erc20::{Erc20Contract, Erc20Interface};
/// pub use crate::defi::blockchain::non_evm::diamond::DiamondBlockchainProvider;
/// ```

/// # Bảng Tra Cứu Module
/// 
/// Bảng tham khảo nhanh để tìm các module quan trọng trong codebase.
/// 
/// ## Core Modules
/// 
/// | Module | Đường dẫn | Mô tả |
/// |--------|-----------|-------|
/// | WalletManagerApi | `wallet::walletmanager::api` | API chính để tương tác với wallet |
/// | WalletManagerHandler | `wallet::walletlogic::handler` | Trait định nghĩa các thao tác với ví |
/// | ChainManager | `wallet::walletmanager::chain` | Quản lý kết nối blockchain |
/// | DefiApi | `wallet::defi::api` | API để tương tác với các chức năng DeFi |
/// | SubscriptionManager | `wallet::users::subscription::manager` | Quản lý subscription và đăng ký |
/// | TokenManager | `wallet::defi::token::manager` | Quản lý thông tin token với cache |
/// | CacheSystem | `wallet::cache` | Hệ thống cache với LRU và TTL |
/// 
/// ## Blockchain Providers
/// 
/// | Provider | Đường dẫn | Mô tả |
/// |----------|-----------|-------|
/// | BlockchainProviderFactory | `wallet::defi::blockchain` | Factory để tạo blockchain providers |
/// | DiamondProvider | `wallet::defi::blockchain::non_evm::diamond` | Provider cho blockchain Diamond |
/// | TronProvider | `wallet::defi::blockchain::non_evm::tron` | Provider cho blockchain Tron |
/// | SolanaProvider | `wallet::defi::blockchain::non_evm::solana` | Provider cho blockchain Solana |
/// | NearProvider | `wallet::defi::blockchain::non_evm::near` | Provider cho blockchain Near |
/// | CosmosProvider | `wallet::defi::blockchain::non_evm::cosmos` | Provider cho blockchain Cosmos |
/// | HederaProvider | `wallet::defi::blockchain::non_evm::hedera` | Provider cho blockchain Hedera |
/// | EvmProvider | `wallet::defi::blockchain` | Provider cho các blockchain EVM |
/// 
/// ## Token Standards
/// 
/// | Standard | Đường dẫn | Mô tả |
/// |----------|-----------|-------|
/// | Erc20Contract | `wallet::defi::erc20` | Implementation của ERC20 token standard |
/// | Erc721Contract | `wallet::defi::erc721` | Implementation của ERC721 NFT standard |
/// | Erc1155Contract | `wallet::defi::erc1155` | Implementation của ERC1155 Multi-Token standard |
/// | ContractInterface | `wallet::defi::contracts` | Giao diện chung cho smart contracts |
/// | ContractRegistry | `wallet::defi::contracts` | Quản lý các smart contracts |
/// 
/// ## User Management
/// 
/// | Module | Đường dẫn | Mô tả |
/// |--------|-----------|-------|
/// | FreeUserManager | `wallet::users::free_user::manager` | Quản lý người dùng miễn phí |
/// | PremiumUserManager | `wallet::users::premium_user` | Quản lý người dùng premium |
/// | VipUserManager | `wallet::users::vip_user` | Quản lý người dùng VIP |
/// | SubscriptionManager | `wallet::users::subscription::manager` | Quản lý subscription |
/// | AutoTradeManager | `wallet::users::subscription::auto_trade` | Quản lý auto-trade |
/// | PaymentProcessor | `wallet::users::subscription::payment` | Xử lý thanh toán |
/// | StakingManager | `wallet::users::subscription::staking` | Quản lý staking |
/// 
/// ## DeFi Components
/// 
/// | Module | Đường dẫn | Mô tả |
/// |--------|-----------|-------|
/// | DefiProvider | `wallet::defi::provider` | Giao diện cho DeFi operations |
/// | FarmManager | `wallet::defi::farm` | Quản lý farming pools |
/// | StakeManager | `wallet::defi::stake` | Quản lý staking pools |
/// | SecurityProvider | `wallet::defi::security` | Các tính năng bảo mật cho DeFi |
/// | TokenManager | `wallet::defi::token::manager` | Quản lý thông tin token |
/// 
/// ## Error Handling
/// 
/// | Module | Đường dẫn | Mô tả |
/// |--------|-----------|-------|
/// | WalletError | `wallet::error` | Các loại lỗi chung cho wallet |
/// | DefiError | `wallet::defi::error` | Các loại lỗi cho DeFi |
/// | ContractError | `wallet::defi::contracts` | Các loại lỗi cho smart contracts |
/// | SubscriptionError | `wallet::users::subscription::types` | Các loại lỗi cho subscription |
/// 
/// ## Testing Utilities
/// 
/// | Module | Đường dẫn | Mô tả |
/// |--------|-----------|-------|
/// | MockWalletHandler | `wallet::users::free_user::test_utils` | Mock cho WalletManagerHandler |
/// | TestUtils | `wallet::tests` | Các tiện ích cho testing |