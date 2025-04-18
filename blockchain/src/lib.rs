//!
//! # Thư viện blockchain cho DiamondChain
//!
//! Thư viện này cung cấp các chức năng để tương tác với các blockchain và smart contracts.
//! Bao gồm các module chính:
//!
//! * `smartcontracts`: Quản lý tương tác với các smart contract trên các blockchain khác nhau
//! * `stake`: Quản lý staking pools và các chức năng liên quan đến staking
//! * `farm`: Quản lý farming pools và các chức năng liên quan đến yield farming
//! * `exchange`: Quản lý các cặp token và liquidity pools trên DEX
//!
//! ## Kiến trúc
//!
//! Thư viện này sử dụng mẫu thiết kế Factory và Provider để cung cấp một abstraction layer
//! trên các blockchain khác nhau. Mỗi blockchain có provider riêng implement các trait chung.
//!
//! ## Các trait chính
//!
//! * `TokenInterface`: Cung cấp các phương thức để tương tác với token trên các blockchain
//! * `BridgeInterface`: Cung cấp các phương thức để bridge token giữa các blockchain
//! * `StakePoolManager`: Cung cấp các phương thức để quản lý stake pools
//!
//! ## Sử dụng
//!
//! ```rust
//! use blockchain::smartcontracts::eth_contract::EthContractProvider;
//! use blockchain::stake::StakeManager;
//! use blockchain::farm::FarmManager;
//!
//! // Tạo provider cho Ethereum
//! let provider = EthContractProvider::new(config);
//!
//! // Tạo stake manager
//! let stake_manager = StakeManager::new(provider.clone());
//!
//! // Tạo farm manager
//! let farm_manager = FarmManager::new();
//! ```

// Module exports
pub mod bridge;
pub mod stake;
pub mod farm;
pub mod smartcontracts;
pub mod exchange;
pub mod oracle;

#[cfg(test)]
mod tests; 