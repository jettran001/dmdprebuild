//! Module Non-EVM - Quản lý các contract trên các blockchain không tương thích EVM

mod solana_contract;
pub use solana_contract::*;

mod near_contract;
pub use near_contract::*;

mod ton_contract;
pub use ton_contract::*;

mod non_evm_shared;
pub use non_evm_shared::*; 