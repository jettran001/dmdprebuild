//! Module farm cung cấp các chức năng yield farming
//!
//! Module này chứa các thành phần chính:
//! - `FarmManager`: Quản lý các farm pools và liquidity
//! - `FarmFactory`: Factory để tạo và quản lý các farm pools
//! - Các cấu trúc dữ liệu liên quan như pool config và user info

mod farm_logic;
mod factorry;

pub use farm_logic::{
    FarmManager,
    FarmPoolConfig,
    UserFarmInfo,
    FarmError,
    FarmResult,
};

pub use factorry::FarmFactory;
