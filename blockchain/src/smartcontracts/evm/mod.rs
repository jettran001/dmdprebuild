//! Module EVM - Quản lý các contract trên các blockchain tương thích EVM

mod ethereum_contract;
pub use ethereum_contract::*;

mod bsc_contract;
pub use bsc_contract::*;

mod polygon_contract;
pub use polygon_contract::*;

mod arbitrum_contract;
pub use arbitrum_contract::*;

mod optimism_contract;
pub use optimism_contract::*;

mod fantom_contract;
pub use fantom_contract::*;

mod avalanche_contract;
pub use avalanche_contract::*;

mod evm_shared;
pub use evm_shared::*; 