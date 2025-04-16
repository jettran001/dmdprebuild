//! Thư viện blockchain cho DiamondChain
//! 
//! Module chứa các tương tác với các blockchain và smart contracts

pub mod smartcontracts;  // Tương tác với các smart contract
pub mod stake;           // Logic staking
pub mod exchange;        // Tương tác với DEX
pub mod farm;            // Yield farming
pub mod brigde;          // Bridge giữa các blockchain (đang phát triển)

#[cfg(test)]
mod tests;
