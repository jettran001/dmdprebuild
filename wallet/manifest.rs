//! 🧭 Entry Point: Đây là manifest chính chứa toàn bộ module của dự án wallet.
//! Mỗi thư mục là một đơn vị rõ ràng: `walletlogic`, `walletmanager`, `defi`, `users`, `error`.
//! Bot hãy bắt đầu từ đây để resolve module path chính xác.
//! Được dùng làm tài liệu tham chiếu khi import từ domain khác (ví dụ: snipebot).

/*
    wallet/
    ├── Cargo.toml                  -> Cấu hình dependencies
    ├── error.rs                   -> Định nghĩa Error (WalletError) và các utility functions, sử dụng thiserror [liên quan: tất cả các module sử dụng WalletError]
    ├── lib.rs                     -> Khai báo module cấp cao [liên quan: tất cả module khác, điểm import cho crate]
    ├── main.rs                    -> Điểm chạy chính, demo WalletManagerApi với ví dụ tạo và import wallet [liên quan: walletmanager::api, config]
    ├── config.rs                  -> Cấu hình ví (WalletSystemConfig với default_chain_id, default_chain_type, max_wallets) [liên quan: walletmanager::api, walletlogic]
    ├── cache.rs                   -> Tiện ích quản lý cache chung với AsyncCache, LRUCache và hàm get_or_load_with_cache [liên quan: các module sử dụng cache như users::subscription::auto_trade]
    ├── manifest.rs                -> Tài liệu tham chiếu module path [liên quan: tất cả các module, BẮT BUỘC đọc đầu tiên]
    ├── src/walletlogic/           -> Logic cốt lõi ví
    │   ├── mod.rs                 -> Khai báo submodule [liên quan: tất cả các file trong walletlogic]
    │   ├── init.rs                -> Khởi tạo WalletManager, tạo/nhập ví với mã hóa seed/key [liên quan: crypto.rs, handler.rs, utils.rs, bao gồm hàm create_wallet_internal, import_wallet_internal]
    │   ├── handler.rs             -> Xử lý ví, định nghĩa trait WalletManagerHandler với các phương thức quản lý ví, ký/gửi giao dịch, kiểm tra số dư [liên quan: utils.rs, crypto.rs, walletmanager::api, bao gồm export_seed_phrase_internal, export_private_key_internal, remove_wallet_internal, update_chain_id_internal, sign_transaction, send_transaction, get_balance]
    │   ├── utils.rs               -> Các hàm tiện ích, định nghĩa UserType (Free, Premium, VIP), tạo user_id, kiểm tra seed phrase [liên quan: init.rs, handler.rs, bao gồm generate_user_id, generate_default_user_id, is_seed_phrase]
    │   ├── crypto.rs              -> Mã hóa/giải mã seed phrase và private key bằng AES-256-GCM và PBKDF2 [liên quan: init.rs, handler.rs, bao gồm encrypt_data và decrypt_data với 100,000 vòng lặp PBKDF2]
    ├── src/walletmanager/         -> API/UI cho ví
    │   ├── api.rs                 -> API công khai gọi walletlogic (create_wallet, import_wallet, export_seed_phrase, export_private_key, remove_wallet, update_chain_id, get_balance, sign_transaction, send_transaction) [liên quan: walletlogic::init, handler, types.rs, chain.rs]
    │   ├── lib.rs                 -> Re-export api, types để tiện import từ bên ngoài [liên quan: api.rs, types.rs]
    │   ├── mod.rs                 -> Khai báo submodule (api, types, chain) [liên quan: tất cả file trong walletmanager]
    │   ├── types.rs               -> Kiểu dữ liệu (SeedLength, WalletSecret, WalletConfig, WalletInfo) [liên quan: api.rs, chain.rs]
    │   ├── chain.rs               -> Quản lý chain (ChainManager trait, DefaultChainManager, SimpleChainManager, ChainConfig, ChainType: EVM, Solana, TON, NEAR, Stellar, Sui, BTC) với async API [liên quan: api.rs, walletlogic::handler]
    ├── src/defi/                  -> Chức năng DeFi (farming, staking)
    │   ├── api.rs                 -> API công khai DefiApi cho DeFi với các phương thức get_farming_opportunities, get_staking_opportunities [liên quan: farm.rs, stake.rs, walletlogic::handler]
    │   ├── farm.rs                -> Logic farming với FarmingManager (add_liquidity, withdraw_liquidity, harvest_rewards) và FarmingOpportunity (pool_address, token_address, apy, tvl) [liên quan: api.rs, walletmanager::api]
    │   ├── stake.rs               -> Logic staking với StakingManager (stake_tokens, unstake_tokens, calculate_rewards) và StakingOpportunity (protocol_name, lock_period, apr) [liên quan: api.rs, walletmanager::api]
    │   ├── lib.rs                 -> Re-export các thành phần chính từ module defi [liên quan: api.rs, farm.rs, stake.rs]
    │   └── mod.rs                 -> Khai báo và re-export submodule DefiApi, FarmingManager, StakingManager [liên quan: tất cả file trong defi]
    ├── src/users/                 -> Quản lý người dùng
    │   ├── mod.rs                 -> Khai báo submodule [liên quan: tất cả folder/file trong users]
    │   ├── free_user/             -> Module người dùng miễn phí
    │   │   ├── mod.rs             -> Khai báo submodule [liên quan: tất cả file trong free_user]
    │   │   ├── types.rs           -> Định nghĩa kiểu dữ liệu FreeUserData, UserStatus, TransactionType [liên quan: manager.rs, auth.rs, limits.rs]
    │   │   ├── manager.rs         -> Quản lý tổng thể người dùng miễn phí (FreeUserManager), sử dụng cache.rs để lưu và truy xuất dữ liệu hiệu quả [liên quan: types.rs, auth.rs, limits.rs, records.rs, queries.rs, cache.rs]
    │   │   ├── auth.rs            -> Xác thực người dùng miễn phí (login, register, verify), sử dụng cache thông qua manager.rs [liên quan: types.rs, manager.rs]
    │   │   ├── limits.rs          -> Kiểm tra và áp dụng giới hạn giao dịch cho người dùng miễn phí [liên quan: types.rs, manager.rs]
    │   │   ├── records.rs         -> Ghi nhận hoạt động của người dùng miễn phí [liên quan: types.rs, manager.rs]
    │   │   ├── queries.rs         -> Truy vấn dữ liệu người dùng miễn phí thông qua cache [liên quan: types.rs, manager.rs, cache.rs]
    │   │   └── test_utils.rs      -> Công cụ kiểm thử với MockWalletHandler [liên quan: tất cả file trong free_user, tests/auth_tests.rs]
    │   ├── subscription/          -> Module quản lý đăng ký người dùng
    │   │   ├── mod.rs             -> Khai báo submodule, re-export SubscriptionManager, StakingManager, các kiểu dữ liệu [liên quan: tất cả file trong subscription]
    │   │   ├── manager.rs         -> Quản lý tổng thể đăng ký (SubscriptionManager), xử lý nâng/hạ cấp gói, xác minh NFT/Staking [liên quan: user_subscription.rs, types.rs, auto_trade.rs, nft.rs, staking.rs, payment.rs, events.rs, walletmanager::api]
    │   │   ├── user_subscription.rs -> Cấu trúc dữ liệu UserSubscription, thực hiện trait SubscriptionConverter để chuyển đổi giữa các loại gói [liên quan: manager.rs, types.rs, auto_trade.rs, nft.rs]
    │   │   ├── types.rs           -> Kiểu dữ liệu đăng ký: SubscriptionType, SubscriptionStatus, Feature, PaymentToken [liên quan: manager.rs, user_subscription.rs, staking.rs]
    │   │   ├── constants.rs       -> Hằng số và tham số cấu hình (stake amounts, APY, giá gói đăng ký) [liên quan: manager.rs, staking.rs, payment.rs]
    │   │   ├── auto_trade.rs      -> Quản lý thời gian auto-trade (AutoTradeManager), theo dõi thời gian sử dụng, sử dụng cache chung từ wallet/cache.rs [liên quan: manager.rs, user_subscription.rs, cache.rs]
    │   │   ├── nft.rs             -> Kiểm tra và xác thực NFT, quản lý VipNftInfo và NonNftVipStatus [liên quan: manager.rs, walletmanager::api, vip_user.rs]
    │   │   ├── staking.rs         -> Quản lý stake DMD token (ERC-1155) cho gói 12 tháng, 30% APY, xử lý TokenStake và StakeStatus [liên quan: manager.rs, types.rs, constants.rs, walletmanager::api]
    │   │   ├── payment.rs         -> Xử lý thanh toán đăng ký, xác minh giao dịch blockchain [liên quan: manager.rs, constants.rs, walletmanager::api]
    │   │   ├── events.rs          -> Phát và quản lý sự kiện đăng ký (SubscriptionEvent, EventType, EventEmitter) [liên quan: manager.rs, user_subscription.rs]
    │   │   ├── utils.rs           -> Các tiện ích và helper function [liên quan: tất cả file trong subscription]
    │   │   └── tests.rs           -> Unit tests cho subscription [liên quan: tất cả file trong subscription]
    │   ├── premium_user.rs        -> Logic người dùng premium: PremiumUserData, PremiumUserStatus, PremiumUserManager, quản lý thanh toán và chu kỳ đăng ký [liên quan: subscription, walletmanager::api, subscription::manager]
    │   ├── vip_user.rs            -> Logic người dùng VIP: VipUserData, VipUserStatus, VipUserManager, quản lý NFT, staking và đặc quyền cao cấp [liên quan: subscription, walletmanager::api, subscription::nft, subscription::staking]
    │   └── subscription.rs        -> (Sắp di chuyển sang thư mục subscription) [liên quan: premium_user.rs, vip_user.rs]
    ├── tests/                     -> Integration tests cho wallet
    │   ├── mod.rs                 -> Khai báo các test modules cho free_user [liên quan: auth_tests.rs, limits_tests.rs, records_tests.rs]
    │   ├── integration.rs         -> Integration tests cho wallet lifecycle, import/export, sign/balance [liên quan: walletmanager::api, walletlogic, config]
    │   ├── auth_tests.rs          -> Tests cho xác thực người dùng, test_verify_free_user [liên quan: users::free_user::auth, free_user::test_utils]
    │   ├── limits_tests.rs        -> Tests cho kiểm tra giới hạn giao dịch [liên quan: users::free_user::limits]
    │   └── records_tests.rs       -> Tests cho ghi nhận hoạt động [liên quan: users::free_user::records]

    Mối liên kết:
    - walletlogic phụ thuộc error (dùng WalletError)
    - walletlogic::crypto được dùng bởi init.rs để mã hóa khóa và bởi handler.rs để giải mã
    - walletlogic::init quản lý WalletManager với wallets: Arc<RwLock<HashMap<Address, WalletInfo>>>
    - walletlogic::init thực hiện tạo và nhập ví, sử dụng utils.rs để tạo user_id
    - walletlogic::handler định nghĩa trait WalletManagerHandler với 12 phương thức quản lý ví
    - walletlogic::handler implement các chức năng như export, remove, update wallet, ký/gửi giao dịch
    - walletlogic::utils cung cấp hàm tạo user_id cho từng loại người dùng (Free, Premium, VIP)
    - walletlogic::crypto sử dụng AES-256-GCM và PBKDF2 với 100,000 vòng lặp cho bảo mật cao
    - walletmanager::api gọi walletlogic::init, handler, dùng config, chain
    - walletmanager::api cung cấp các phương thức công khai như create_wallet, import_wallet, export_seed_phrase, export_private_key, remove_wallet, update_chain_id, get_balance, sign_transaction, send_transaction
    - walletmanager::api sử dụng DefaultChainManager để quản lý các provider kết nối đến blockchain
    - walletmanager::chain định nghĩa trait ChainManager với API async nhất quán và các cài đặt DefaultChainManager, SimpleChainManager
    - walletmanager::chain hỗ trợ nhiều loại blockchain: EVM, Solana, TON, NEAR, Stellar, Sui, BTC
    - walletmanager::types định nghĩa các cấu trúc dữ liệu như SeedLength, WalletSecret, WalletConfig, WalletInfo
    - defi có thể dùng walletlogic::handler để truy xuất ví
    - defi::api cung cấp DefiApi với các phương thức get_farming_opportunities, get_staking_opportunities
    - defi::farm định nghĩa FarmingManager cho quản lý hoạt động farming (add/withdraw liquidity, harvest)
    - defi::stake định nghĩa StakingManager cho quản lý staking (stake/unstake tokens, calculate rewards)
    - config cung cấp WalletSystemConfig với default_chain_id, max_wallets và phương thức can_add_wallet
    - error định nghĩa WalletError sử dụng thiserror với 11 loại lỗi khác nhau và implement Clone
    - users có thể liên kết với walletlogic qua user_id
    - users::free_user cung cấp chức năng cơ bản cho người dùng miễn phí với giới hạn giao dịch
    - users::free_user::manager quản lý hạn chế giao dịch, lưu lịch sử hoạt động
    - users::free_user::manager sử dụng cache.rs để lưu trữ và truy xuất thông tin người dùng hiệu quả
    - users::premium_user quản lý người dùng premium với PremiumUserManager và PremiumUserData
    - users::premium_user.rs hỗ trợ nâng cấp từ free lên premium, gia hạn đăng ký
    - users::vip_user.rs cung cấp VipUserManager với đặc quyền cao cấp nhất, kiểm tra NFT
    - users::vip_user.rs hỗ trợ upgrade_with_staking để sử dụng DMD token thay thế NFT
    - users::subscription::manager là trung tâm điều phối, kết nối tất cả các module subscription
    - users::subscription::manager tương tác với wallet_api, free_user_manager, premium_user_manager
    - users::subscription::user_subscription định nghĩa cấu trúc cốt lõi UserSubscription
    - users::subscription::user_subscription cung cấp trait SubscriptionConverter để chuyển đổi giữa các gói
    - users::subscription::nft phụ thuộc vào walletmanager để kiểm tra NFT
    - users::subscription::staking quản lý TokenStake, StakeStatus cho việc stake DMD token
    - users::subscription::payment phụ thuộc vào blockchain để xác minh giao dịch
    - users::subscription::auto_trade quản lý thời gian sử dụng auto_trade cho từng loại gói
    - users::subscription::auto_trade sử dụng cache.rs để lưu trữ và truy xuất thông tin auto_trade hiệu quả
    - users::subscription::events gửi thông báo khi có thay đổi subscription
    - users::subscription::types định nghĩa SubscriptionType, SubscriptionStatus, Feature, PaymentToken
    - users::subscription::staking phụ thuộc vào walletmanager::api để tương tác với blockchain
    - users::subscription::staking quản lý DMD token chuẩn ERC-1155 và cung cấp 30% APY hàng năm
    - users::subscription::manager::stake_tokens_for_subscription yêu cầu bắt buộc có NFT trong ví
    - users::subscription::manager::verify_all_nft_ownership bỏ qua kiểm tra NFT cho gói 12 tháng stake
    - users::subscription::manager và users::subscription::auto_trade sử dụng Weak references để tránh circular references
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
pub mod defi;          // DeFi functionality (farming, staking)
pub mod users;         // Quản lý người dùng và đăng ký

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