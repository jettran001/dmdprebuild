//! 🧭 Entry Point: Đây là manifest chính chứa toàn bộ module của dự án wallet.
//! Mỗi thư mục là một đơn vị rõ ràng: `walletlogic`, `walletmanager`, `defi`, `users`, `error`.
//! Bot hãy bắt đầu từ đây để resolve module path chính xác.
//! Được dùng làm tài liệu tham chiếu khi import từ domain khác (ví dụ: snipebot).

/*
    wallet/
    ├── error.rs                    -> Định nghĩa Error dùng chung (WalletError)
    ├── lib.rs                      -> Khai báo các module cấp cao của wallet
    ├── main.rs                     -> Điểm chạy chính, ví dụ sử dụng walletlogic
    ├── registry/                   -> Chứa manifest và registry config
    │   └── manifest.rs             -> Tài liệu tham chiếu cho module path
    ├── src/walletlogic/            -> Logic cốt lõi về ví, dùng rộng cho các module
    │   ├── mod.rs                  -> Khai báo submodule của walletlogic
    │   ├── init.rs                 -> Khởi tạo WalletManager, tạo/nhập ví
    │   ├── handler.rs              -> Xử lý transaction/wallet ops (xuất, xóa, truy xuất)
    │   ├── utils.rs                -> Hàm helper (validate, gen seed, ký)
    ├── src/walletmanager/          -> Xử lý riêng cho UI/API wallet
    │   ├── api.rs                  -> Hàm công khai gọi walletlogic (UI/API)
    │   ├── config.rs               -> Cấu hình ví (hiện để trống)
    │   ├── lib.rs                  -> Re-export các thành phần công khai
    │   ├── mod.rs                  -> Khai báo submodule của walletmanager
    │   ├── types.rs                -> Các kiểu dữ liệu (WalletConfig, WalletInfo)
    ├── src/defi/                   -> Chức năng DeFi (farming, staking)
    │   ├── api.rs                  -> API công khai cho DeFi
    │   ├── farm.rs                 -> Logic farming
    │   ├── stake.rs                -> Logic staking
    │   ├── lib.rs                  -> Re-export các thành phần công khai
    │   └── mod.rs                  -> Khai báo submodule của defi
    ├── src/users/                  -> Quản lý người dùng (free, premium, vip)
    │   ├── free_user.rs            -> Logic cho người dùng miễn phí
    │   ├── premium_user.rs         -> Logic cho người dùng premium
    │   ├── vip_user.rs             -> Logic cho người dùng VIP
    │   ├── lib.rs                  -> Re-export các thành phần công khai
    │   └── mod.rs                  -> Khai báo submodule của users

    Mối liên kết:
    - walletlogic phụ thuộc error (dùng WalletError)
    - walletmanager::api gọi walletlogic::init và walletlogic::handler
    - walletmanager dùng walletmanager::types
    - defi có thể dùng walletlogic::handler để truy xuất ví
    - users có thể liên kết với walletlogic qua user_id trong WalletInfo
    - main.rs dùng walletlogic để demo chức năng
*/

// Khai báo các module cấp cao
pub mod error;
pub mod walletlogic;
pub mod walletmanager;
pub mod defi;
pub mod users;