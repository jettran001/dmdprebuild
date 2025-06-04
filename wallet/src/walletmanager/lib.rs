// wallet/src/walletmanager/lib.rs
//! Module re-export cho các thành phần quan trọng của walletmanager.
//! 
//! Module này cung cấp các API chính cho việc quản lý ví, blockchain và cấu hình.
//! Mục đích chính là giúp các module khác dễ dàng import các thành phần cần thiết
//! từ walletmanager mà không cần chỉ định đường dẫn đầy đủ.

pub use api::*;
pub use types::*;
pub use chain::*;
