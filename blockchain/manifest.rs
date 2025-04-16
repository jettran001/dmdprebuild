//! 🧭 Entry Point: Đây là manifest chính chứa toàn bộ module của dự án blockchain.
//! Mỗi thư mục là một đơn vị rõ ràng: `smartcontracts`, `stake`, `exchange`, `farm`, `brigde`.
//! Bot hãy bắt đầu từ đây để resolve module path chính xác.
//! Được dùng làm tài liệu tham chiếu khi import từ domain khác (ví dụ: wallet, snipebot).

/*
    blockchain/
    ├── Cargo.toml                  -> Cấu hình dependencies
    ├── manifest.rs                 -> Tài liệu tham chiếu module path [liên quan: tất cả các module, BẮT BUỘC đọc đầu tiên]
    ├── lib.rs                      -> Khai báo module cấp cao, re-export [liên quan: tất cả module khác, điểm import cho crate]
    ├── tests.rs                    -> Unit tests cho toàn bộ module blockchain [liên quan: tất cả module]
    ├── src/smartcontracts/         -> Tương tác với các smart contract
    │   ├── mod.rs                  -> Khai báo submodule smartcontracts, định nghĩa TokenInterface và BridgeInterface [liên quan: tất cả file trong smartcontracts]
    │   ├── dmd_token.rs            -> Tương tác với DMD Token (ERC-1155), quản lý tier và token allowlist [liên quan: stake, exchange]
    │   ├── dmd_bsc_bridge.rs       -> Bridge DMD token giữa BSC và các chain khác [liên quan: bsc_contract, near_contract]
    │   ├── bsc_contract/           -> Tương tác với DMD token trên BSC (ERC-1155)
    │   │   ├── mod.rs              -> Cung cấp BscContractProvider và thông tin về contract DMD token [liên quan: crate::smartcontracts::TokenInterface]
    │   │   ├── contract.rs         -> Mã nguồn của contract DMD token trên BSC [liên quan: mod.rs]
    │   │   ├── dmd_bsc_contract.sol -> Solidity code cho DMD token trên BSC [liên quan: contract.rs]
    │   │   ├── abi.json            -> ABI của contract DMD token trên BSC [liên quan: mod.rs]
    │   ├── near_contract/          -> Tương tác với DMD token trên NEAR
    │   │   ├── mod.rs              -> Cung cấp NearContractProvider [liên quan: crate::smartcontracts::TokenInterface]
    │   │   ├── smartcontract.rs    -> Cung cấp kiểu DmdChain và các hàm liên quan [liên quan: mod.rs]
    │   ├── solana_contract/        -> Tương tác với DMD token trên Solana
    │   │   ├── mod.rs              -> Cung cấp SolanaContractProvider [liên quan: crate::smartcontracts::TokenInterface]
    │   │   ├── smartcontract.rs    -> Định nghĩa các hàm tương tác với Solana [liên quan: mod.rs]
    │   ├── eth_contract/           -> Tương tác với DMD token trên Ethereum
    │   │   ├── mod.rs              -> Cung cấp EthContractProvider và thông tin về contract DMD token [liên quan: crate::smartcontracts::TokenInterface]
    │   │   ├── dmd_eth_contract.sol -> Solidity code cho DMD token trên Ethereum [liên quan: mod.rs]
    │   ├── arb_contract/           -> Tương tác với DMD token trên Arbitrum
    │   │   ├── mod.rs              -> Cung cấp ArbContractProvider và thông tin về contract DMD token [liên quan: crate::smartcontracts::TokenInterface]
    │   │   ├── DiamondTokenARB.sol -> Solidity code cho DMD token trên Arbitrum [liên quan: mod.rs]
    │   │   ├── external/           -> Thư viện và dependencies bên ngoài cho Arbitrum [liên quan: DiamondTokenARB.sol]
    │   ├── base_contract/          -> Tương tác với DMD token trên Base L2
    │   │   ├── DiamondTokenETH.sol -> Solidity code cho DMD token trên Base [liên quan: mod.rs]
    │   │   ├── dmd_eth_contract.sol -> Solidity code cho DMD token trên Ethereum [liên quan: DiamondTokenETH.sol]
    │   │   ├── dmd_base_contract.sol -> Solidity code cho DMD token trên Base [liên quan: mod.rs]
    │   │   ├── external/           -> Thư viện và dependencies bên ngoài cho Base [liên quan: dmd_base_contract.sol]
    │   ├── polygon_contract/       -> Tương tác với DMD token trên Polygon
    │   │   ├── mod.rs              -> Cung cấp PolygonContractProvider [liên quan: crate::smartcontracts::TokenInterface]
    ├── src/stake/                  -> Logic staking
    │   ├── mod.rs                  -> Khai báo submodule stake, re-export StakeManager và FarmManager [liên quan: tất cả file trong stake]
    │   ├── stake_logic.rs          -> Logic staking chính, định nghĩa StakeManager [liên quan: smartcontracts, dmd_token]
    │   ├── farm_logic.rs           -> Logic farm liên quan đến staking, định nghĩa FarmManager [liên quan: farm, smartcontracts]
    │   ├── constants.rs            -> Các hằng số cho staking [liên quan: stake_logic, farm_logic]
    │   ├── validator.rs            -> Validator cho proof-of-stake [liên quan: smartcontracts] (đang triển khai)
    │   ├── rewards.rs              -> Tính toán và phân phối phần thưởng [liên quan: smartcontracts, dmd_token] (đang triển khai)
    │   ├── routers.rs              -> Routers cho các pool staking [liên quan: smartcontracts] (đang triển khai)
    ├── src/exchange/               -> Tương tác với DEX
    │   ├── pairs.rs                -> Quản lý token pairs và liquidity pools [liên quan: smartcontracts] (đang triển khai)
    ├── src/farm/                   -> Yield farming
    │   ├── mod.rs                  -> Khai báo submodule farm [liên quan: tất cả file trong farm]
    │   ├── farm_logic.rs           -> Logic farming chính [liên quan: stake, smartcontracts]
    │   ├── factorry.rs             -> Factory cho các farm mới [liên quan: smartcontracts, exchange] (đang triển khai)
    ├── src/brigde/                 -> Bridge giữa các blockchain (đang phát triển)

    Mối liên kết:
    - smartcontracts là trung tâm tương tác với blockchain
    - smartcontracts/mod.rs định nghĩa các trait chính: TokenInterface và BridgeInterface
    - smartcontracts/dmd_token.rs quản lý tương tác với DMD token (ERC-1155), hệ thống tier và danh sách token được phép
    - smartcontracts/dmd_bsc_bridge.rs quản lý bridge DMD token giữa BSC và các blockchain khác
    - smartcontracts/bsc_contract/ quản lý tương tác với DMD token trên BSC
    - smartcontracts/near_contract/ quản lý tương tác với DMD token trên NEAR
    - smartcontracts/solana_contract/ quản lý tương tác với DMD token trên Solana
    - smartcontracts/eth_contract/ quản lý tương tác với DMD token trên Ethereum
    - smartcontracts/arb_contract/ quản lý tương tác với DMD token trên Arbitrum
    - smartcontracts/base_contract/ quản lý tương tác với DMD token trên Base L2
    - smartcontracts/polygon_contract/ quản lý tương tác với DMD token trên Polygon
    - stake/mod.rs là cổng vào cho tất cả chức năng staking
    - stake/stake_logic.rs chứa logic staking chính
    - stake/farm_logic.rs chứa logic farm liên quan đến staking
    - stake/constants.rs chứa các hằng số cho staking
    - farm/mod.rs là cổng vào cho tất cả chức năng farming
    - farm/farm_logic.rs chứa logic farming chính
    - exchange/pairs.rs quản lý token pairs và quản lý liquidity pools (đang triển khai)
    - blockchain tương tác với wallet để thực hiện giao dịch
    - blockchain cung cấp API cho snipebot để thực hiện giao dịch
    - smartcontracts định nghĩa các trait quan trọng như TokenInterface và BridgeInterface
*/

// Module structure của dự án blockchain
pub mod smartcontracts;  // Tương tác với các smart contract
pub mod stake;           // Logic staking
pub mod exchange;        // Tương tác với DEX
pub mod farm;            // Yield farming
pub mod brigde;          // Bridge giữa các blockchain (đang phát triển)

/**
 * Hướng dẫn import:
 * 
 * 1. Import từ internal crates:
 * - use crate::smartcontracts::dmd_token::DMDToken;
 * - use crate::smartcontracts::dmd_bsc_bridge::DmdBscBridge;
 * - use crate::smartcontracts::bsc_contract::BscContractProvider;
 * - use crate::smartcontracts::near_contract::NearContractProvider;
 * - use crate::smartcontracts::solana_contract::SolanaContractProvider; 
 * - use crate::smartcontracts::eth_contract::EthContractProvider;
 * - use crate::smartcontracts::arb_contract::ArbContractProvider;
 * - use crate::smartcontracts::polygon_contract::PolygonContractProvider;
 * - use crate::stake::stake_logic::StakeManager;
 * - use crate::stake::farm_logic::FarmManager;
 * - use crate::farm::farm_logic::FarmLogic;
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
 * - use tokio::time::{sleep, Duration};
 * - use async_trait::async_trait;
 * - use anyhow::{Result, Context};
 */

// Các trait chính trong blockchain domain:
/**
 * TokenInterface (smartcontracts/mod.rs):
 * - Trait cho tương tác với token trên các blockchain khác nhau
 * - Được implement bởi: BscContractProvider, NearContractProvider, SolanaContractProvider, EthContractProvider, ArbContractProvider, PolygonContractProvider
 * - Phương thức chính: get_balance, transfer, total_supply, decimals, token_name, token_symbol, get_total_supply, bridge_to
 * 
 * BridgeInterface (smartcontracts/mod.rs):
 * - Trait cho chức năng bridge token giữa các blockchain
 * - Được implement bởi: DmdBscBridge
 * - Phương thức chính: bridge_tokens, check_bridge_status, estimate_bridge_fee
 */

// Các cập nhật quan trọng:
/**
 * 07-06-2023: Thêm module bsc_contract, near_contract, solana_contract để tương tác với DMD token trên các blockchain khác nhau
 * 08-06-2023: Thêm cơ chế retry và fallback cho BscContractProvider::get_total_supply()
 * 09-06-2023: Sửa định dạng doc comment trong contract.rs để theo chuẩn Rust
 * 10-06-2023: Làm rõ về decimals() trong BscContractProvider
 * 11-06-2023: Thêm cơ chế đảm bảo tính đồng bộ giữa contract.rs và dmd_bsc_contract.sol thông qua SHA-256 hash
 * 12-06-2023: Di chuyển tests.rs từ bsc_contract lên cấp cao hơn để tạo thành một bộ test tích hợp cho toàn bộ module blockchain
 * 13-06-2023: Sửa địa chỉ token trên các blockchain khác nhau trong DmdConfig::new() để mỗi blockchain có địa chỉ riêng
 * 13-06-2023: Thay URL RPC Ethereum placeholder bằng URL thật không cần API key
 * 13-06-2023: Thêm cơ chế cache và retry cho NearContractProvider::get_total_supply()
 * 14-06-2023: Cải thiện BscContractProvider::get_total_supply() với cơ chế cache tương tự như NearContractProvider
 * 15-06-2023: Thêm module arb_contract để tương tác với DMD token trên Arbitrum
 * 16-06-2023: Thêm module base_contract để tương tác với DMD token trên Base L2
 * 17-06-2023: Tối ưu hóa cơ chế cache cho ArbContractProvider cho việc xử lý tầng (tier) token
 * 18-06-2023: Thêm module polygon_contract để tương tác với DMD token trên Polygon
 * 22-06-2023: Thêm farm_logic.rs trong module stake để quản lý logic farm liên quan đến staking
 * 23-06-2023: Thêm constants.rs trong module stake để quản lý các hằng số
 * 24-06-2023: Thêm stake_logic.rs trong module stake để quản lý logic staking chính
 * 25-06-2023: Thêm farm_logic.rs trong module farm để quản lý logic farming chính
 * 26-06-2023: Tạo module brigde để phát triển các chức năng bridge giữa các blockchain
 * 27-06-2023: Cập nhật manifest.rs để phản ánh cấu trúc thực tế của dự án
 * 28-06-2023: Thêm phương thức has_required_tier vào BscContractProvider để kiểm tra quyền người dùng dựa trên tier
 * 29-06-2023: Cải thiện cơ chế fallback RPC URL cho ArbContractProvider khi kết nối bị lỗi
 * 30-06-2023: Cập nhật TokenInterface để thêm các phương thức liên quan đến kiểm tra balance và bridge tokens
 * 01-07-2023: Thêm phương thức is_token_allowed vào dmd_token.rs để kiểm tra quyền sử dụng token dựa trên tier
 * 02-07-2023: Thêm thư mục external trong arb_contract và base_contract để quản lý dependencies bên ngoài
 * 03-07-2023: Cập nhật smartcontract.rs trong solana_contract để hỗ trợ tương tác với Solana
 * 04-07-2023: Bổ sung các trait bounds Send + Sync + 'static cho TokenInterface và BridgeInterface
 * 05-07-2023: Thêm phương thức bridge_to trong TokenInterface để hỗ trợ bridge tokens giữa các blockchain
 */
