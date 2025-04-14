//! 🧭 Entry Point: Đây là manifest chính chứa toàn bộ module của dự án blockchain.
//! Mỗi thư mục là một đơn vị rõ ràng: `smartcontracts`, `stake`, `exchange`, `farm`.
//! Bot hãy bắt đầu từ đây để resolve module path chính xác.
//! Được dùng làm tài liệu tham chiếu khi import từ domain khác (ví dụ: wallet, snipebot).

/*
    blockchain/
    ├── Cargo.toml                  -> Cấu hình dependencies
    ├── manifest.rs                 -> Tài liệu tham chiếu module path [liên quan: tất cả các module, BẮT BUỘC đọc đầu tiên]
    ├── lib.rs                      -> Khai báo module cấp cao, re-export [liên quan: tất cả module khác, điểm import cho crate]
    ├── src/smartcontracts/         -> Tương tác với các smart contract
    │   ├── mod.rs                  -> Khai báo submodule smartcontracts [liên quan: tất cả file trong smartcontracts]
    │   ├── dmd_token.rs            -> Tương tác với DMD Token (ERC-1155) [liên quan: stake, exchange]
    │   ├── diamond_nft.rs          -> Tương tác với Diamond NFT [liên quan: wallet::users::subscription::nft]
    │   ├── factory.rs              -> Smart contract factory cho các token mới [liên quan: exchange, farm]
    │   ├── router.rs               -> Smart contract router cho DEX [liên quan: exchange]
    │   ├── pair.rs                 -> Smart contract pair cho DEX [liên quan: exchange]
    ├── src/stake/                  -> Logic staking
    │   ├── mod.rs                  -> Khai báo submodule stake [liên quan: tất cả file trong stake]
    │   ├── routers.rs              -> Routers cho các pool staking [liên quan: smartcontracts]
    │   ├── rewards.rs              -> Tính toán và phân phối phần thưởng [liên quan: smartcontracts, dmd_token]
    │   ├── validator.rs            -> Validator cho proof-of-stake [liên quan: smartcontracts]
    ├── src/exchange/               -> Tương tác với DEX
    │   ├── pairs.rs                -> Quản lý token pairs và liquidity pools [liên quan: smartcontracts]
    │   ├── swap.rs                 -> Logic swap tokens [liên quan: smartcontracts, wallet]
    │   ├── liquidity.rs            -> Quản lý liquidity (add/remove) [liên quan: smartcontracts, wallet]
    ├── src/farm/                   -> Yield farming
    │   ├── factorry.rs             -> Factory cho các farm mới [liên quan: smartcontracts, exchange]
    │   ├── rewards.rs              -> Phân phối reward cho farming [liên quan: smartcontracts, dmd_token]
    │   ├── pools.rs                -> Quản lý pools farming [liên quan: stake, exchange, smartcontracts]

    Mối liên kết:
    - smartcontracts là trung tâm tương tác với blockchain
    - smartcontracts/dmd_token.rs quản lý tương tác với DMD token (ERC-1155)
    - smartcontracts/diamond_nft.rs quản lý NFT VIP cho hệ thống subscription
    - smartcontracts/factory.rs tạo và quản lý các token mới
    - smartcontracts/router.rs và pair.rs tương tác với DEX
    - stake/mod.rs là cổng vào cho tất cả chức năng staking
    - stake/routers.rs quản lý các pool staking, giao tiếp với các smart contract
    - stake/rewards.rs tính toán và phân phối phần thưởng từ staking
    - stake/validator.rs kiểm tra tính hợp lệ trong hệ thống proof-of-stake
    - exchange/pairs.rs quản lý token pairs và quản lý liquidity pools
    - exchange/swap.rs cung cấp logic cho việc swap tokens
    - exchange/liquidity.rs quản lý việc thêm và rút liquidity
    - farm/factorry.rs tạo và quản lý các farm mới
    - farm/rewards.rs phân phối phần thưởng từ farming
    - farm/pools.rs quản lý các pool farming
    - blockchain tương tác với wallet để thực hiện giao dịch
    - blockchain cung cấp API cho snipebot để thực hiện giao dịch
    - smartcontracts/dmd_token.rs được wallet::users::subscription::staking sử dụng để quản lý DMD token
    - smartcontracts/diamond_nft.rs được wallet::users::subscription::nft sử dụng để xác minh NFT VIP
*/

// Module structure của dự án blockchain
pub mod smartcontracts;  // Tương tác với các smart contract
pub mod stake;           // Logic staking
pub mod exchange;        // Tương tác với DEX
pub mod farm;            // Yield farming

/**
 * Hướng dẫn import:
 * 
 * 1. Import từ internal crates:
 * - use crate::smartcontracts::dmd_token::DMDToken;
 * - use crate::smartcontracts::diamond_nft::DiamondNFT;
 * - use crate::stake::routers::StakingRouter;
 * - use crate::stake::rewards::RewardCalculator;
 * - use crate::exchange::pairs::LiquidityPool;
 * - use crate::exchange::swap::SwapExecutor;
 * - use crate::farm::factorry::FarmFactory;
 * - use crate::farm::pools::FarmPool;
 * 
 * 2. Import từ external crates:
 * - use wallet::walletmanager::api::WalletManagerApi;
 * - use wallet::users::subscription::staking::StakingManager;
 * - use wallet::users::subscription::nft::NftValidator;
 * - use snipebot::chain_adapters::evm_adapter::EvmAdapter;
 * 
 * 3. Import từ third-party libraries:
 * - use ethers::providers::{Provider, Http};
 * - use ethers::contract::Contract;
 * - use ethers::types::{Address, U256, Transaction};
 * - use ethers::abi::{Abi, Function, Token};
 */
