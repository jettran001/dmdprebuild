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
    │   ├── free_user.rs           -> Logic người dùng miễn phí
    │   ├── premium_user.rs        -> Logic người dùng premium
    │   ├── vip_user.rs            -> Logic người dùng VIP
    │   ├── lib.rs                 -> Re-export
    │   └── mod.rs                 -> Khai báo submodule

    Mối liên kết:
    - walletlogic phụ thuộc error (dùng WalletError)
    - walletlogic::crypto được dùng bởi init.rs để mã hóa khóa
    - walletmanager::api gọi walletlogic::init, handler, dùng config, chain
    - walletmanager dùng walletmanager::types, chain
    - defi có thể dùng walletlogic::handler để truy xuất ví
    - users có thể liên kết với walletlogic qua user_id
    - main.rs dùng walletmanager và config để demo
*/

// Khai báo các module cấp cao
pub mod error;
pub mod config;
pub mod walletlogic;
pub mod walletmanager;
pub mod defi;
pub mod users;