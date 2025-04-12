//! 🧭 Entry Point: Đây là manifest chính chứa toàn bộ module của dự án wallet.
//! Mỗi thư mục là một đơn vị rõ ràng: `walletmanager`, `defi`, `users`, `error`.
//! Bot hãy bắt đầu từ đây để resolve module path chính xác.
//! Được dùng làm tài liệu tham chiếu khi import từ domain khác (ví dụ: snipebot).

/*
    wallet/
    ├── error.rs                    -> Định nghĩa Error dùng chung cho wallet (WalletError)
    ├── lib.rs                      -> Khai báo các module cấp cao của wallet
    ├── config.rs               -> Cấu hình ví (hiện để trống, sẵn sàng mở rộng)
    ├── main.rs                     -> Điểm chạy chính, ví dụ sử dụng walletmanager
    ├── manifest.rs                 -> Tài liệu tham chiếu cho module path
    ├── src/walletmanager/          -> Quản lý ví (tạo, nhập, xuất, xóa ví)
    │   ├── api.rs                  -> Các hàm công khai để thao tác ví (create_wallet, import_wallet)
    │   ├── lib.rs                  -> Re-export các thành phần công khai của walletmanager
    │   ├── mod.rs                  -> Khai báo submodule của walletmanager
    │   ├── types.rs                -> Các kiểu dữ liệu (WalletConfig, WalletSecret, WalletInfo)
    │   └── walletlogic.rs          -> Logic nội bộ của WalletManager (quản lý HashMap, user_id)
    ├── src/defi/                   -> Chức năng DeFi (farming, staking)
    │   ├── api.rs                  -> Các API công khai cho DeFi (farm, stake)
    │   ├── farm.rs                 -> Logic farming
    │   ├── stake.rs                -> Logic staking
    │   ├── lib.rs                  -> Re-export các thành phần công khai của defi
    │   └── mod.rs                  -> Khai báo submodule của defi
    ├── src/users/                  -> Quản lý người dùng (free, premium, vip)
    │   ├── free_user.rs            -> Logic cho người dùng miễn phí
    │   ├── premium_user.rs         -> Logic cho người dùng premium
    │   ├── vip_user.rs             -> Logic cho người dùng VIP
    │   ├── lib.rs                  -> Re-export các thành phần công khai của users
    │   └── mod.rs                  -> Khai báo submodule của users

    Mối liên kết:
    - walletmanager phụ thuộc error (dùng WalletError)
    - walletmanager::api gọi walletmanager::walletlogic và dùng walletmanager::types
    - defi có thể dùng walletmanager::api để truy xuất ví
    - users có thể liên kết với walletmanager qua user_id trong WalletInfo
    - main.rs dùng walletmanager để demo chức năng
*/

// Khai báo các module cấp cao
pub mod error;
pub mod walletmanager;
pub mod defi;
pub mod users;