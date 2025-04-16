//! Module quản lý các hoạt động staking và farming trong blockchain
//! 
//! Module này bao gồm các chức năng:
//! - Quản lý staking pools và rewards
//! - Quản lý farming pools, liquidity, và rewards
//! - Tính toán APY và rewards
//! - Giao tiếp với các smart contract staking

pub mod stake_logic;
pub mod farm_logic;
pub mod routers;
pub mod validator;
pub mod rewards;

pub use stake_logic::StakeManager;
pub use farm_logic::FarmManager;
