//! 🧭 Entry Point: Đây là manifest chính chứa toàn bộ module của dự án wallet.
//! Mỗi thư mục là một đơn vị rõ ràng: `walletlogic`, `walletmanager`, `defi`, `users`, `error`.
//! Bot hãy bắt đầu từ đây để resolve module path chính xác.
//! Được dùng làm tài liệu tham chiếu khi import từ domain khác (ví dụ: snipebot).

/*
    wallet/
    ├── Cargo.toml                  -> Cấu hình dependencies
    ├── error.rs                   -> Định nghĩa Error (WalletError)
    ├── lib.rs                     -> Khai báo module cấp cao
    ├── main.rs                    -> Điểm chạy chính, demo WalletManagerApi
    ├── config.rs                  -> Cấu hình ví (chain_id, chain_type, max_wallets)
    ├── manifest.rs                -> Tài liệu tham chiếu module path
    ├── src/walletlogic/           -> Logic cốt lõi ví
    │   ├── mod.rs                 -> Khai báo submodule
    │   ├── init.rs                -> Khởi tạo WalletManager, tạo/nhập ví
    │   ├── handler.rs             -> Xử lý ví, ký/gửi giao dịch, số dư
    │   ├── utils.rs               -> Hàm helper (validate, gen seed, ký)
    │   ├── crypto.rs              -> Mã hóa seed/private key bằng mật khẩu
    ├── src/walletmanager/         -> API/UI cho ví
    │   ├── api.rs                 -> API công khai gọi walletlogic
    │   ├── lib.rs                 -> Re-export api, types
    │   ├── mod.rs                 -> Khai báo submodule
    │   ├── types.rs               -> Kiểu dữ liệu (WalletConfig, WalletInfo)
    │   ├── chain.rs               -> Quản lý chain (EVM, Solana, TON, v.v.)
    ├── src/defi/                  -> Chức năng DeFi (farming, staking)
    │   ├── api.rs                 -> API công khai cho DeFi
    │   ├── farm.rs                -> Logic farming
    │   ├── stake.rs               -> Logic staking
    │   ├── lib.rs                 -> Re-export
    │   └── mod.rs                 -> Khai báo submodule
    ├── src/users/                 -> Quản lý người dùng
    │   ├── mod.rs                 -> Khai báo submodule
    │   ├── free_user/             -> Module người dùng miễn phí
    │   │   ├── mod.rs             -> Khai báo submodule
    │   │   ├── types.rs           -> Định nghĩa kiểu dữ liệu
    │   │   ├── manager.rs         -> Quản lý tổng thể
    │   │   ├── auth.rs            -> Xác thực người dùng
    │   │   ├── limits.rs          -> Kiểm tra và áp dụng giới hạn
    │   │   ├── records.rs         -> Ghi nhận hoạt động
    │   │   ├── queries.rs         -> Truy vấn dữ liệu
    │   │   └── test_utils.rs      -> Công cụ kiểm thử
    │   ├── subscription/          -> Module quản lý đăng ký người dùng
    │   │   ├── mod.rs             -> Khai báo submodule
    │   │   ├── manager.rs         -> Quản lý tổng thể đăng ký
    │   │   ├── user_subscription.rs -> Cấu trúc dữ liệu đăng ký
    │   │   ├── types.rs           -> Kiểu dữ liệu đăng ký (sử dụng StakeStatus từ staking.rs)
    │   │   ├── constants.rs       -> Hằng số và tham số cấu hình (stake amounts, APY, giá gói)
    │   │   ├── auto_trade.rs      -> Quản lý thời gian auto-trade
    │   │   ├── nft.rs             -> Kiểm tra và xác thực NFT
    │   │   ├── staking.rs         -> Quản lý stake DMD token (ERC-1155) cho gói 12 tháng, 30% APY
    │   │   ├── payment.rs         -> Xử lý thanh toán đăng ký
    │   │   ├── events.rs          -> Phát và quản lý sự kiện đăng ký
    │   │   ├── utils.rs           -> Các tiện ích và helper function
    │   │   └── tests.rs           -> Unit tests cho subscription
    │   ├── premium_user.rs        -> Logic người dùng premium
    │   ├── vip_user.rs            -> Logic người dùng VIP
    │   └── subscription.rs        -> (Sắp di chuyển sang thư mục subscription)
    ├── tests/                     -> Integration tests
    │   ├── mod.rs                 -> Khai báo các test modules
    │   ├── integration.rs         -> Integration tests cho wallet
    │   ├── auth_tests.rs          -> Tests cho xác thực người dùng
    │   ├── limits_tests.rs        -> Tests cho kiểm tra giới hạn
    │   └── records_tests.rs       -> Tests cho ghi nhận hoạt động

    Mối liên kết:
    - walletlogic phụ thuộc error (dùng WalletError)
    - walletlogic::crypto được dùng bởi init.rs để mã hóa khóa
    - walletmanager::api gọi walletlogic::init, handler, dùng config, chain
    - walletmanager dùng walletmanager::types, chain
    - defi có thể dùng walletlogic::handler để truy xuất ví
    - users có thể liên kết với walletlogic qua user_id
    - users::subscription::manager phụ thuộc vào các user manager khác
    - users::subscription::nft phụ thuộc vào walletmanager để kiểm tra NFT
    - users::subscription::payment phụ thuộc vào blockchain để xác minh giao dịch
    - users::subscription::events gửi thông báo khi có thay đổi subscription
    - users::subscription::staking phụ thuộc vào walletmanager::api để tương tác với blockchain
    - users::subscription::staking quản lý DMD token chuẩn ERC-1155 và cung cấp 30% APY hàng năm
    - users::subscription::manager::stake_tokens_for_subscription yêu cầu bắt buộc có NFT trong ví
    - users::subscription::manager::verify_all_nft_ownership bỏ qua kiểm tra NFT cho gói 12 tháng stake
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
 * - use crate::users::subscription::manager::SubscriptionManager;
 * - use crate::users::subscription::staking::{StakingManager, TokenStake, StakeStatus};
 * 
 * 2. Import từ external crates (từ snipebot hoặc blockchain):
 * - use wallet::walletmanager::api::WalletManagerApi;
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