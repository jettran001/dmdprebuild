//! 🧭 Entry Point: Đây là manifest chính chứa toàn bộ module của dự án blockchain.
//! Mỗi thư mục là một đơn vị rõ ràng: `smartcontracts`, `stake`, `exchange`, `farm`, `bridge`, `oracle`.
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
    ├── src/bridge/                 -> Bridge giữa các blockchain
    │   ├── mod.rs                  -> Khai báo submodule bridge, re-export các thành phần chính [liên quan: tất cả file trong bridge]
    │   ├── bridge.rs               -> Thực hiện chính cho bridge [liên quan: smartcontracts, oracle]
    │   ├── manager.rs              -> Quản lý các giao dịch bridge [liên quan: transaction, oracle]
    │   ├── near_hub.rs             -> Logic bridge với NEAR làm trung tâm [liên quan: bridge, evm_spoke]
    │   ├── evm_spoke.rs            -> Logic bridge từ/đến các EVM chains [liên quan: bridge, near_hub]
    │   ├── transaction.rs          -> Quản lý thông tin giao dịch bridge [liên quan: types, error]
    │   ├── types.rs                -> Các kiểu dữ liệu chung cho module bridge [liên quan: error]
    │   ├── error.rs                -> Custom error types cho module bridge [liên quan: types]
    │   ├── traits.rs               -> Các trait chính định nghĩa giao diện bridge [liên quan: types, error]
    │   ├── oracle.rs               -> Theo dõi và đồng bộ dữ liệu giữa các blockchain [liên quan: oracle module]
    ├── src/oracle/                 -> Oracle cho lấy dữ liệu từ các nguồn bên ngoài
    │   ├── mod.rs                  -> Khai báo submodule oracle, re-export các thành phần chính [liên quan: tất cả file trong oracle]
    │   ├── onchain.rs              -> Oracle lấy dữ liệu từ onchain [liên quan: types, error]
    │   ├── chainlink.rs            -> Tích hợp với Chainlink [liên quan: provider, types]
    │   ├── provider.rs             -> Provider interface cho oracle [liên quan: types]
    │   ├── types.rs                -> Các kiểu dữ liệu cho oracle [liên quan: error]
    │   ├── error.rs                -> Custom error types cho module oracle [liên quan: types]

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
    - bridge/mod.rs là cổng vào cho tất cả chức năng bridge giữa các blockchain
    - bridge/ triển khai mô hình hub and spoke với NEAR Protocol làm trung tâm
    - oracle/mod.rs cung cấp các dịch vụ oracle để lấy dữ liệu từ các nguồn bên ngoài
    - oracle/chainlink.rs tích hợp với Chainlink để lấy dữ liệu đáng tin cậy
    - blockchain tương tác với wallet để thực hiện giao dịch
    - blockchain cung cấp API cho snipebot để thực hiện giao dịch
    - smartcontracts định nghĩa các trait quan trọng như TokenInterface và BridgeInterface
*/

// Module structure của dự án blockchain
pub mod smartcontracts;  // Tương tác với các smart contract
pub mod stake;           // Logic staking
pub mod exchange;        // Tương tác với DEX
pub mod farm;            // Yield farming
pub mod bridge;          // Bridge giữa các blockchain
pub mod oracle;          // Oracle cho lấy dữ liệu từ các nguồn bên ngoài

/**
 * ===========================================================================================
 * TẤT CẢ TRAITS VÀ EXPORTS CHÍNH TRONG DOMAIN BLOCKCHAIN
 * ===========================================================================================
 * 
 * 1. TRAIT CHÍNH
 * --------------
 * 
 * TokenInterface (smartcontracts/mod.rs):
 * - Mô tả: Trait cho tương tác với token trên các blockchain khác nhau
 * - Được implement bởi: BscContractProvider, NearContractProvider, SolanaContractProvider, 
 *   EthContractProvider, ArbContractProvider, PolygonContractProvider
 * - Phương thức chính:
 *   - async fn get_balance(&self, address: &str) -> Result<U256>
 *   - async fn transfer(&self, private_key: &str, to: &str, amount: U256) -> Result<String>
 *   - async fn total_supply(&self) -> Result<U256>
 *   - fn decimals(&self) -> u8
 *   - fn token_name(&self) -> String
 *   - fn token_symbol(&self) -> String
 *   - async fn get_total_supply(&self) -> Result<f64>
 *   - async fn bridge_to(&self, to_chain: &str, from_account: &str, to_account: &str, amount: f64) -> Result<String>
 * 
 * BridgeInterface (smartcontracts/mod.rs):
 * - Mô tả: Trait cho chức năng bridge token giữa các blockchain
 * - Được implement bởi: DmdBscBridge
 * - Phương thức chính:
 *   - async fn bridge_tokens(&self, private_key: &str, to_address: &str, amount: U256) -> Result<String>
 *   - async fn check_bridge_status(&self, tx_hash: &str) -> Result<String>
 *   - async fn estimate_bridge_fee(&self, to_address: &str, amount: U256) -> Result<(U256, U256)>
 * 
 * StakePoolManager (stake/mod.rs):
 * - Mô tả: Trait cho quản lý stake pool
 * - Phương thức chính:
 *   - async fn create_pool(&self, config: StakePoolConfig) -> Result<Address>
 *   - async fn add_token(&self, pool_address: Address, token_address: Address, token_parameters: TokenParameters) -> Result<()>
 *   - async fn get_pool_info(&self, pool_address: Address) -> Result<StakePoolConfig>
 *   - async fn stake(&self, pool_address: Address, user_address: Address, amount: U256, lock_time: u64) -> Result<()>
 *   - async fn unstake(&self, pool_address: Address, user_address: Address) -> Result<()>
 *   - async fn claim_rewards(&self, pool_address: Address, user_address: Address) -> Result<U256>
 *   - async fn get_user_stake_info(&self, pool_address: Address, user_address: Address) -> Result<UserStakeInfo>
 * 
 * StakeManagerFactory (stake/mod.rs):
 * - Mô tả: Factory để tạo ra các stake manager, cung cấp dependency injection
 * - Phương thức chính:
 *   - fn create_stake_manager(&self) -> Arc<dyn StakePoolManager>
 *   - fn create_farm_manager(&self) -> Arc<dyn StakePoolManager>
 *   - fn create_validator(&self) -> Arc<dyn Validator>
 *   - fn create_reward_calculator(&self) -> Arc<dyn RewardCalculator>
 *   - fn create_staking_router(&self) -> Arc<dyn StakingRouter>
 * 
 * StakeConfig (stake/mod.rs):
 * - Mô tả: Trait cho cấu hình stake, cung cấp các thông số cấu hình cho StakeManager
 * - Phương thức chính:
 *   - fn get_cache_ttl(&self) -> u64
 *   - fn get_chain_id(&self) -> u64
 *   - fn get_rpc_url(&self) -> String
 *   - fn get_default_wallet(&self) -> String
 *   - fn get_keystore_path(&self) -> String
 *   - async fn get_gas_price(&self) -> Result<U256>
 *   - fn get_gas_limit(&self) -> U256
 * 
 * Validator (stake/validator.rs):
 * - Mô tả: Trait cho validator trong proof-of-stake
 * - Phương thức chính:
 *   - async fn register_validator(&self, pool_address: Address, validator_address: Address) -> Result<()>
 *   - async fn unregister_validator(&self, pool_address: Address, validator_address: Address) -> Result<()>
 *   - async fn validate_transaction(&self, pool_address: Address, transaction_hash: &str) -> Result<bool>
 *   - async fn get_validators(&self, pool_address: Address) -> Result<Vec<ValidatorInfo>>
 *   - async fn get_validator_info(&self, pool_address: Address, validator_address: Address) -> Result<ValidatorInfo>
 *   - async fn distribute_validator_rewards(&self, pool_address: Address) -> Result<()>
 * 
 * RewardCalculator (stake/rewards.rs):
 * - Mô tả: Trait cho tính toán phần thưởng trong staking
 * - Phương thức chính:
 *   - async fn calculate_pending_rewards(&self, pool: &StakePoolConfig, stake_info: &UserStakeInfo) -> Result<U256>
 *   - async fn distribute_rewards(&self, pool_address: Address, user_address: Address) -> Result<U256>
 * 
 * StakingRouter (stake/routers.rs):
 * - Mô tả: Trait cho router trong staking, xử lý giao tiếp với smart contract
 * - Phương thức chính:
 *   - async fn create_pool(&self, config: StakePoolConfig) -> Result<String>
 *   - async fn add_token_to_pool(&self, pool_address: Address, token_address: Address, token_parameters: TokenParameters) -> Result<String>
 *   - async fn get_pool_config(&self, pool_address: Address) -> Result<StakePoolConfig>
 *   - async fn stake_tokens(&self, pool_address: Address, user_address: Address, amount: U256, lock_time: u64) -> Result<String>
 *   - async fn unstake_tokens(&self, pool_address: Address, user_address: Address) -> Result<String>
 *   - async fn claim_rewards(&self, pool_address: Address, user_address: Address) -> Result<String>
 * 
 * 2. STRUCT CHÍNH VÀ EXPORT
 * --------------------------
 * 
 * smartcontracts/dmd_token.rs:
 * - DMDToken: Struct chính để tương tác với DMD Token (ERC-1155)
 * - DmdConfig: Cấu hình cho DMD Token
 * 
 * smartcontracts/bsc_contract/mod.rs:
 * - BscContractProvider: Provider cho BSC blockchain
 * - BscContractConfig: Cấu hình cho BscContractProvider
 * - DmdBscContract: Thông tin về DMD token trên BSC
 * 
 * stake/mod.rs:
 * - StakeManager: Quản lý stake pool và rewards
 * - FarmManager: Quản lý farm liên quan đến staking
 * - StakePoolConfig: Cấu hình cho stake pool
 * - UserStakeInfo: Thông tin stake của người dùng
 * - StakePoolCache: Cache cho stake pools
 * - TokenParameters: Cấu hình tokenomics cho token trong pool
 * - DefaultStakeManagerFactory: Triển khai mặc định của StakeManagerFactory
 * - DefaultStakeConfig: Triển khai mặc định của StakeConfig
 * 
 * stake/stake_logic.rs:
 * - StakeManager: Triển khai của StakePoolManager
 * - StakeError: Custom error types cho module stake
 * 
 * farm/farm_logic.rs:
 * - FarmManager: Quản lý hệ thống farming
 * - FarmPoolConfig: Cấu hình của farming pool
 * - UserFarmInfo: Thông tin farm của người dùng
 * - FarmError: Enum chứa các lỗi có thể xảy ra trong quá trình farming
 * 
 * 3. API VÀ ENTRY POINTS CHÍNH
 * ----------------------------
 * 
 * DMDToken (smartcontracts/dmd_token.rs):
 * - new(): Tạo mới DMDToken instance 
 * - get_token_tier(address: &str): Lấy tier của token
 * - is_token_allowed(address: &str, required_tier: u8): Kiểm tra token có được phép sử dụng dựa trên tier
 * 
 * DmdBscBridge (smartcontracts/dmd_bsc_bridge.rs):
 * - new(): Tạo mới DmdBscBridge instance
 * - bridge_tokens(): Bridge token từ BSC sang chain khác
 * 
 * StakeManager (stake/stake_logic.rs):
 * - new(): Tạo mới StakeManager
 * - with_cache_ttl(): Tạo StakeManager với TTL tùy chỉnh
 * - create_pool(): Tạo pool staking mới
 * - add_token(): Thêm token vào pool với các tham số
 * - stake(): Stake token vào pool
 * - unstake(): Unstake token từ pool
 * - claim_rewards(): Nhận rewards từ staking
 * - refresh_cache_if_needed(): Cập nhật cache nếu cần
 * - invalidate_cache(): Xóa cache cho pool cụ thể
 * - clear_all_cache(): Xóa toàn bộ cache
 * 
 * DefaultStakeManagerFactory (stake/mod.rs):
 * - new(): Tạo factory mới với config mặc định
 * - with_config(): Tạo factory với config tùy chỉnh
 * - create_stake_manager(): Tạo StakeManager mới
 * - create_farm_manager(): Tạo FarmManager mới
 * - create_validator(): Tạo Validator mới
 * - create_reward_calculator(): Tạo RewardCalculator mới
 * - create_staking_router(): Tạo StakingRouter mới
 * 
 * FarmManager (farm/farm_logic.rs):
 * - new(): Tạo mới FarmManager
 * - add_pool(): Thêm farming pool mới
 * - add_liquidity(): Thêm liquidity vào pool
 * - remove_liquidity(): Rút liquidity từ pool
 * - harvest(): Thu rewards từ farming
 * 
 * 4. HƯỚNG DẪN IMPORT
 * -------------------
 * 
 * Import từ internal crates:
 * - use crate::smartcontracts::dmd_token::DMDToken;
 * - use crate::smartcontracts::dmd_bsc_bridge::DmdBscBridge;
 * - use crate::smartcontracts::bsc_contract::BscContractProvider;
 * - use crate::smartcontracts::near_contract::NearContractProvider;
 * - use crate::smartcontracts::solana_contract::SolanaContractProvider; 
 * - use crate::smartcontracts::eth_contract::EthContractProvider;
 * - use crate::smartcontracts::arb_contract::ArbContractProvider;
 * - use crate::smartcontracts::polygon_contract::PolygonContractProvider;
 * - use crate::stake::StakeManager;
 * - use crate::stake::FarmManager;
 * - use crate::stake::{StakePoolManager, StakeManagerFactory, DefaultStakeManagerFactory};
 * - use crate::farm::farm_logic::FarmLogic;
 * 
 * Import từ external crates:
 * - use wallet::walletmanager::api::WalletManagerApi;
 * - use wallet::users::subscription::staking::StakingManager;
 * - use wallet::users::subscription::nft::NftValidator;
 * - use snipebot::chain_adapters::evm_adapter::EvmAdapter;
 * 
 * Import từ third-party libraries:
 * - use ethers::providers::{Provider, Http};
 * - use ethers::contract::Contract;
 * - use ethers::types::{Address, U256, Transaction};
 * - use ethers::abi::{Abi, Function, Token};
 * - use tokio::time::{sleep, Duration};
 * - use async_trait::async_trait;
 * - use anyhow::{Result, Context};
 * - use thiserror::Error;
 * - use lazy_static::lazy_static;
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
 * 20-07-2023: Cập nhật manifest.rs với thông tin chi tiết về traits, exports và APIs cho sử dụng từ các domain khác
 * 10-08-2023: Cải thiện module stake/constants.rs để tải cấu hình từ biến môi trường thay vì giá trị cứng trong mã nguồn
 * 11-08-2023: Thêm custom error types trong stake/stake_logic.rs để cải thiện xử lý lỗi
 * 12-08-2023: Cải thiện validator.rs để kiểm tra đầy đủ quyền hạn trước khi đăng ký validator
 * 13-08-2023: Bổ sung validate address trước khi tạo pool trong stake_logic.rs
 * 14-08-2023: Triển khai dependency injection cho các thành phần trong stake/mod.rs với StakeManagerFactory và StakeConfig
 * 15-08-2023: Thêm TokenParameters trong stake/mod.rs để cấu hình tokenomics cho token trong pool
 * 16-08-2023: Tạo module oracle để theo dõi và đồng bộ dữ liệu giữa các blockchain
 * 17-08-2023: Cập nhật bridge/mod.rs để sử dụng mô hình hub and spoke với NEAR Protocol làm trung tâm
 * 18-08-2023: Thêm module oracle/chainlink.rs để tích hợp với Chainlink oracles
 * 19-08-2023: Thêm bridge/oracle.rs để tận dụng oracle trong quá trình bridging
 * 20-08-2023: Triển khai oracle/onchain.rs để theo dõi sự kiện và trạng thái trên blockchain
 * 21-08-2023: Cải thiện bridge/transaction.rs để hỗ trợ theo dõi các giao dịch bridge xuyên chain
 * 22-08-2023: Triển khai giao diện OracleProvider trong oracle/provider.rs
 * 23-08-2023: Thêm các kiểu dữ liệu oracle trong oracle/types.rs
 * 24-08-2023: Cập nhật manifest.rs để thêm thông tin về module bridge và oracle
 * 25-08-2023: Thêm kiểm tra số dư trong bridge/evm_spoke.rs::send_to_hub trước khi thực hiện giao dịch
 * 26-08-2023: Cải thiện phương thức update_token_price trong bridge/oracle.rs để xác thực dữ liệu đầu vào
 * 27-08-2023: Triển khai cơ chế xác thực chéo (cross-validation) trong bridge/oracle.rs để phòng chống double-spent
 * 28-08-2023: Thêm phương thức detect_abnormal_transaction vào bridge/oracle.rs để phát hiện giao dịch bất thường
 * 29-08-2023: Cải thiện phương thức wait_for_transaction_confirmation trong bridge/evm_spoke.rs với cơ chế timeout
 * 30-08-2023: Thêm cơ chế dọn dẹp cache tự động (manage_cache) trong bridge/near_hub.rs
 * 31-08-2023: Triển khai cross_validate_bridge_transaction trong bridge/oracle.rs để xác thực chéo các giao dịch bridge
 * 01-09-2023: Bổ sung các phương thức validate_transaction_risk vào bridge/bridge.rs cho LayerZeroAdapter
 */

/**
 * ===========================================================================================
 * TẤT CẢ TRAITS VÀ EXPORTS CHÍNH TRONG DOMAIN BLOCKCHAIN
 * ===========================================================================================
 * 
 * 1. TRAIT CHÍNH
 * --------------
 * 
 * TokenInterface (smartcontracts/mod.rs):
 * - Mô tả: Trait cho tương tác với token trên các blockchain khác nhau
 * - Được implement bởi: BscContractProvider, NearContractProvider, SolanaContractProvider, 
 *   EthContractProvider, ArbContractProvider, PolygonContractProvider
 * - Phương thức chính:
 *   - async fn get_balance(&self, address: &str) -> Result<U256>
 *   - async fn transfer(&self, private_key: &str, to: &str, amount: U256) -> Result<String>
 *   - async fn total_supply(&self) -> Result<U256>
 *   - fn decimals(&self) -> u8
 *   - fn token_name(&self) -> String
 *   - fn token_symbol(&self) -> String
 *   - async fn get_total_supply(&self) -> Result<f64>
 *   - async fn bridge_to(&self, to_chain: &str, from_account: &str, to_account: &str, amount: f64) -> Result<String>
 * 
 * BridgeInterface (smartcontracts/mod.rs):
 * - Mô tả: Trait cho chức năng bridge token giữa các blockchain
 * - Được implement bởi: DmdBscBridge
 * - Phương thức chính:
 *   - async fn bridge_tokens(&self, private_key: &str, to_address: &str, amount: U256) -> Result<String>
 *   - async fn check_bridge_status(&self, tx_hash: &str) -> Result<String>
 *   - async fn estimate_bridge_fee(&self, to_address: &str, amount: U256) -> Result<(U256, U256)>
 * 
 * BridgeHub (bridge/traits.rs):
 * - Mô tả: Trait cho hub trong mô hình bridge hub-and-spoke
 * - Được implement bởi: NearBridgeHub
 * - Phương thức chính:
 *   - async fn init_bridge(&self) -> BridgeResult<()>
 *   - async fn bridge_to_spoke(&self, transaction: &BridgeTransaction) -> BridgeResult<H256>
 *   - async fn validate_spoke_bridge(&self, transaction_hash: &H256) -> BridgeResult<BridgeStatus>
 *   - async fn finalize_spoke_bridge(&self, transaction_hash: &H256) -> BridgeResult<()>
 * 
 * BridgeSpoke (bridge/traits.rs):
 * - Mô tả: Trait cho spoke trong mô hình bridge hub-and-spoke
 * - Được implement bởi: EvmBridgeSpoke
 * - Phương thức chính:
 *   - async fn init_bridge(&self) -> BridgeResult<()>
 *   - async fn bridge_to_hub(&self, transaction: &BridgeTransaction) -> BridgeResult<H256>
 *   - async fn validate_hub_bridge(&self, transaction_hash: &H256) -> BridgeResult<BridgeStatus>
 *   - async fn finalize_hub_bridge(&self, transaction_hash: &H256) -> BridgeResult<()>
 * 
 * OracleProvider (oracle/provider.rs):
 * - Mô tả: Trait cho các oracle provider
 * - Được implement bởi: ChainlinkOracle
 * - Phương thức chính:
 *   - async fn get_price(&self, token_symbol: &str) -> Result<f64>
 *   - async fn get_data(&self, data_feed: &str) -> Result<OracleData>
 *   - async fn subscribe(&self, data_feed: &str, callback: Box<dyn Fn(OracleData) + Send + Sync + 'static>) -> Result<()>
 *   - async fn unsubscribe(&self, subscription_id: &str) -> Result<()>
 *
 * BridgeOracleManager (bridge/oracle.rs):
 * - Mô tả: Quản lý oracle dữ liệu cho bridge và phát hiện giao dịch bất thường
 * - Phương thức chính:
 *   - async fn detect_abnormal_transaction(&self, source_chain: &str, target_chain: &str, amount: &str, sender: &str) -> BridgeResult<bool>
 *   - async fn cross_validate_bridge_transaction(&self, source_chain: &str, target_chain: &str, tx_hash: &TransactionHash, amount: &str, sender: &str, recipient: &str) -> BridgeResult<bool>
 *   - async fn validate_token_price(&self, price: f64) -> BridgeResult<()>
 *   - async fn detect_suspicious_pattern(&self, transactions: &[BridgeTransactionData]) -> BridgeResult<bool>
 *   - fn get_chain_transaction_limits(&self, source_chain: &str, target_chain: &str) -> (f64, f64)
 * 
 * // ... existing code ...
 */

/**
 * FLOW GỌI TỪ DOMAIN KHÁC VÀO BLOCKCHAIN:
 * ---------------------------------------
 * 
 * 1. Từ wallet -> blockchain:
 *    - Wallet gọi các phương thức trong TokenInterface để tương tác với DMD token
 *    - Wallet sử dụng BridgeInterface để chuyển token giữa các blockchain
 *    - Wallet sử dụng bridge::BridgeManager để quản lý và theo dõi các giao dịch bridge
 *    - Wallet sử dụng oracle::OracleProvider để lấy dữ liệu giá cả và thông tin thị trường
 * 
 * 2. Từ snipebot -> blockchain:
 *    - Snipebot sử dụng TokenInterface để kiểm tra balance, thông tin token
 *    - Snipebot gọi các phương thức trong BscContractProvider, EthContractProvider để tương tác với blockchain cụ thể
 *    - Snipebot sử dụng oracle::ChainlinkOracle để lấy dữ liệu giá chính xác cho các quyết định giao dịch
 * 
 * 3. Từ diamond_manager -> blockchain:
 *    - Diamond manager sử dụng StakeManager để quản lý staking
 *    - Diamond manager sử dụng FarmManager để quản lý farming
 *    - Diamond manager sử dụng bridge::BridgeManager để theo dõi và quản lý bridge transaction
 *    - Diamond manager sử dụng bridge::oracle để phát hiện và ngăn chặn giao dịch bridge bất thường
 */
