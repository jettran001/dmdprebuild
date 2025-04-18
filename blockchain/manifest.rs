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
    ├── src/
    │   ├── lib.rs                  -> Entry point cho crate library
    │   ├── bridge/
    │   │   ├── mod.rs
    │   │   ├── bridge.rs
    │   │   ├── sol_hub.rs
    │   │   ├── oracle.rs
    │   │   ├── evm_spoke.rs
    │   │   ├── config.rs
    │   │   ├── persistent_repository.rs
    │   │   ├── transaction.rs
    │   │   ├── near_hub.rs
    │   │   ├── manager.rs
    │   │   ├── error.rs
    │   │   ├── traits.rs
    │   │   ├── types.rs
    │   ├── smartcontracts/
    │   │   ├── mod.rs
    │   │   ├── dmd_token.rs
    │   │   ├── dmd_bsc_bridge.rs
    │   │   ├── arb_contract/
    │   │   │   ├── mod.rs
    │   │   │   ├── DiamondTokenARB.sol
    │   │   │   ├── external/
    │   │   ├── base_contract/
    │   │   │   ├── DiamondTokenETH.sol
    │   │   │   ├── dmd_eth_contract.sol
    │   │   │   ├── dmd_base_contract.sol
    │   │   │   ├── external/
    │   │   ├── eth_contract/
    │   │   │   ├── mod.rs
    │   │   │   ├── dmd_eth_contract.sol
    │   │   ├── polygon_contract/
    │   │   │   ├── mod.rs
    │   │   ├── solana_contract/
    │   │   │   ├── mod.rs
    │   │   │   ├── smartcontract.rs
    │   │   ├── bsc_contract/
    │   │   │   ├── mod.rs
    │   │   │   ├── dmd_bsc_contract.sol
    │   │   ├── near_contract/
    │   │   │   ├── mod.rs
    │   │   │   ├── smartcontract.rs
    │   │   ├── stake/
    │   │   │   ├── mod.rs
    │   │   │   ├── stake_logic.rs
    │   │   │   ├── farm_logic.rs
    │   │   │   ├── constants.rs
    │   │   │   ├── validator.rs
    │   │   │   ├── rewards.rs
    │   │   │   ├── routers.rs
    │   │   ├── farm/
    │   │   │   ├── mod.rs
    │   │   │   ├── farm_logic.rs
    │   │   │   ├── factorry.rs
    │   │   ├── exchange/
    │   │   │   ├── pairs.rs
    │   │   ├── oracle/
    │   │   │   ├── mod.rs
    │   │   │   ├── onchain.rs
    │   │   │   ├── chainlink.rs
    │   │   │   ├── provider.rs
    │   │   │   ├── types.rs
    │   │   │   ├── error.rs
*/

// Mối liên kết:
// - smartcontracts là trung tâm tương tác với blockchain
// - smartcontracts/mod.rs định nghĩa các trait chính: TokenInterface và BridgeInterface
// - smartcontracts/dmd_token.rs quản lý tương tác với DMD token (ERC-1155), hệ thống tier và danh sách token được phép
// - smartcontracts/dmd_bsc_bridge.rs quản lý bridge DMD token giữa BSC và các blockchain khác
// - smartcontracts/bsc_contract/ quản lý tương tác với DMD token trên BSC
// - smartcontracts/near_contract/ quản lý tương tác với DMD token trên NEAR
// - smartcontracts/solana_contract/ quản lý tương tác với DMD token trên Solana
// - smartcontracts/eth_contract/ quản lý tương tác với DMD token trên Ethereum
// - smartcontracts/arb_contract/ quản lý tương tác với DMD token trên Arbitrum
// - smartcontracts/base_contract/ quản lý tương tác với DMD token trên Base L2
// - smartcontracts/polygon_contract/ quản lý tương tác với DMD token trên Polygon
// - stake/mod.rs là cổng vào cho tất cả chức năng staking
// - stake/stake_logic.rs chứa logic staking chính
// - stake/farm_logic.rs chứa logic farm liên quan đến staking
// - stake/constants.rs chứa các hằng số cho staking
// - farm/mod.rs là cổng vào cho tất cả chức năng farming
// - farm/farm_logic.rs chứa logic farming chính
// - exchange/pairs.rs quản lý token pairs và quản lý liquidity pools (đang triển khai)
// - bridge/mod.rs là cổng vào cho tất cả chức năng bridge giữa các blockchain
// - bridge/ triển khai mô hình hub and spoke với NEAR Protocol làm trung tâm
// - oracle/mod.rs cung cấp các dịch vụ oracle để lấy dữ liệu từ các nguồn bên ngoài
// - oracle/chainlink.rs tích hợp với Chainlink để lấy dữ liệu đáng tin cậy
// - blockchain tương tác với wallet để thực hiện giao dịch
// - blockchain cung cấp API cho snipebot để thực hiện giao dịch
// - smartcontracts định nghĩa các trait quan trọng như TokenInterface và BridgeInterface

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

/// Mô tả các thành phần của hệ thống - Được hợp nhất từ blockchain/src/manifest.rs
pub mod components {
    /// Module Bridge - Quản lý chuyển token giữa các blockchain khác nhau
    pub mod bridge {
        /// Phiên bản hiện tại của module bridge
        pub const VERSION: &str = "0.3.5"; // Tăng phiên bản từ 0.3.4 lên 0.3.5
        
        /// Mô tả: Bridge module cho phép chuyển token giữa các blockchain khác nhau,
        /// sử dụng mô hình hub-and-spoke với NEAR Protocol làm hub trung tâm.
        /// Module được thiết kế để hỗ trợ nhiều blockchain khác nhau thông qua các adapter.
        /// 
        /// Cập nhật mới:
        /// - Thêm persistent storage cho Bridge Transactions để đảm bảo dữ liệu không bị mất sau khi ứng dụng khởi động lại
        /// - Cải thiện bảo mật cho private keys trong BridgeConfig với mã hóa và quản lý khóa an toàn hơn
        /// - Thêm validation cho tất cả các giao dịch bridge trước khi thực hiện
        /// - Thêm kiểm tra số dư trước khi gửi tokens để tránh giao dịch thất bại
        /// - Tối ưu hóa phương thức find_adapter để tăng hiệu suất
        /// - Cải thiện quản lý bộ nhớ cache với các cơ chế dọn dẹp tự động
        /// - Refactor adapter code để dễ dàng mở rộng và hỗ trợ thêm blockchain mới
        /// - Thêm cơ chế cross-validation giữa các giao dịch bridge
        /// - Cải thiện logging để dễ dàng debug hơn
        /// - Thêm concurrency control để tránh race condition
        /// - Thêm is_valid_address để xác thực định dạng địa chỉ ví cho các blockchain
        /// - Cải thiện hệ thống kiểm soát và phát hiện giao dịch đáng ngờ
        /// - Tăng cường xác thực trong phương thức complete_transaction và fail_transaction
        /// - Thêm các chức năng theo dõi và xử lý giao dịch bất thường
        pub struct BridgeManifest;
        
        /// Các tính năng chính của Bridge module
        pub mod features {
            /// Chuyển token từ EVM chain đến NEAR (hub)
            pub const BRIDGE_EVM_TO_NEAR: &str = "bridge_evm_to_near";
            
            /// Chuyển token từ NEAR (hub) đến EVM chain
            pub const BRIDGE_NEAR_TO_EVM: &str = "bridge_near_to_evm";
            
            /// Xem trạng thái giao dịch bridge
            pub const CHECK_TRANSACTION_STATUS: &str = "check_transaction_status";
            
            /// Quản lý bridge adapters 
            pub const MANAGE_ADAPTERS: &str = "manage_adapters";
            
            /// Lưu trữ liên tục cho bridge transactions
            pub const PERSISTENT_STORAGE: &str = "persistent_storage";
            
            /// Validation xác thực chéo giữa các giao dịch
            pub const CROSS_TRANSACTION_VALIDATION: &str = "cross_transaction_validation";
            
            /// Phát hiện giao dịch bất thường
            pub const ANOMALY_DETECTION: &str = "anomaly_detection";
            
            /// Quản lý bộ nhớ cache tự động
            pub const AUTO_CACHE_MANAGEMENT: &str = "auto_cache_management";
            
            /// Kiểm tra số dư trước khi thực hiện giao dịch
            pub const BALANCE_VERIFICATION: &str = "balance_verification";
            
            /// Xác thực định dạng địa chỉ ví
            pub const ADDRESS_VALIDATION: &str = "address_validation";
            
            /// Hệ thống phát hiện và xử lý giao dịch đáng ngờ
            pub const SUSPICIOUS_TRANSACTION_DETECTION: &str = "suspicious_transaction_detection";
            
            /// Xử lý batch cho các giao dịch bridge
            pub const BATCH_TRANSACTION_PROCESSING: &str = "batch_transaction_processing";
            
            /// Giới hạn giao dịch đồng thời
            pub const CONCURRENT_TRANSACTION_LIMITING: &str = "concurrent_transaction_limiting";
        }
    }
    
    /// Module Oracle - Cung cấp dữ liệu giá và thông tin từ các nguồn tin cậy
    pub mod oracle {
        /// Phiên bản hiện tại của module oracle
        pub const VERSION: &str = "0.2.8"; // Tăng phiên bản từ 0.2.7 lên 0.2.8
        
        /// Mô tả: Oracle module cung cấp dữ liệu giá token và thông tin
        /// từ nhiều nguồn đáng tin cậy. Module này đảm bảo tính chính xác 
        /// của dữ liệu thông qua cơ chế đồng thuận và phát hiện dữ liệu bất thường.
        /// 
        /// Cập nhật mới:
        /// - Thêm cơ chế đồng thuận (consensus) cho multiple data providers
        /// - Triển khai phương thức xử lý ngoại lệ (outlier) cho dữ liệu giá
        /// - Thêm validation cho token price để đảm bảo giá không âm, không quá lớn hoặc quá nhỏ
        /// - Thêm phương thức detect_abnormal_transaction để nhận diện giao dịch bất thường
        /// - Thêm cross-validation logic cho các bridge transaction
        /// - Thêm các phương thức kiểm tra transaction validity, sender reputation, và suspicious relationships
        /// - Cải thiện các phương thức phát hiện mối quan hệ đáng ngờ giữa người gửi và người nhận
        /// - Thêm các phương thức kiểm tra giới hạn giao dịch theo cặp blockchain
        /// - Cải thiện kiểm tra validation cho các giao dịch xuyên chuỗi
        /// - Tăng cường tính bảo mật với việc phát hiện các mô hình giao dịch đáng ngờ (smurfing)
        pub struct OracleManifest;
        
        /// Các tính năng chính của Oracle module
        pub mod features {
            /// Cập nhật giá token từ nhiều nguồn
            pub const UPDATE_TOKEN_PRICE: &str = "update_token_price";
            
            /// Kiểm tra giá consensus dựa trên nhiều nguồn dữ liệu
            pub const VERIFY_PRICE_CONSENSUS: &str = "verify_price_consensus";
            
            /// Quản lý các provider dữ liệu
            pub const MANAGE_PROVIDERS: &str = "manage_providers";
            
            /// Phát hiện và xử lý dữ liệu ngoại lệ
            pub const HANDLE_OUTLIERS: &str = "handle_outliers";
            
            /// Phát hiện giao dịch bất thường
            pub const DETECT_ABNORMAL_TRANSACTION: &str = "detect_abnormal_transaction";
            
            /// Xác thực chéo các giao dịch bridge
            pub const CROSS_VALIDATE_TRANSACTIONS: &str = "cross_validate_transactions";
            
            /// Kiểm tra uy tín của người gửi
            pub const CHECK_SENDER_REPUTATION: &str = "check_sender_reputation";
            
            /// Phát hiện các mối quan hệ đáng ngờ
            pub const DETECT_SUSPICIOUS_RELATIONSHIPS: &str = "detect_suspicious_relationships";
            
            /// Kiểm tra giới hạn giao dịch theo cặp blockchain
            pub const CHECK_CHAIN_TRANSACTION_LIMITS: &str = "check_chain_transaction_limits";
            
            /// Phân tích lịch sử giao dịch của người gửi
            pub const ANALYZE_SENDER_HISTORY: &str = "analyze_sender_history";
            
            /// Theo dõi và phát hiện giao dịch đáng ngờ (smurfing)
            pub const DETECT_SUSPICIOUS_PATTERN: &str = "detect_suspicious_pattern";
            
            /// Kiểm tra hợp lệ của token price
            pub const VALIDATE_TOKEN_PRICE: &str = "validate_token_price";
        }
    }
    
    /// Module Wallet - Quản lý ví và tương tác với blockchain
    pub mod wallet {
        /// Version hiện tại của Wallet module
        pub const VERSION: &str = "0.4.1";
        
        /// Mô tả: Wallet module quản lý tất cả các tương tác với blockchain,
        /// bao gồm tạo và quản lý ví, ký giao dịch, và tương tác với smart contracts.
        /// Module này kết nối chặt chẽ với bridge để hỗ trợ giao dịch xuyên chuỗi an toàn.
        pub struct WalletManifest;
        
        /// Các tính năng chính của Wallet module
        pub mod features {
            /// Tạo ví mới
            pub const CREATE_WALLET: &str = "create_wallet";
            
            /// Khôi phục ví từ seed phrase
            pub const RESTORE_WALLET: &str = "restore_wallet";
            
            /// Ký giao dịch
            pub const SIGN_TRANSACTION: &str = "sign_transaction";
            
            /// Tương tác với smart contracts
            pub const INTERACT_WITH_CONTRACTS: &str = "interact_with_contracts";
            
            /// Lấy số dư ví
            pub const GET_BALANCE: &str = "get_balance";
            
            /// Lấy lịch sử giao dịch
            pub const GET_TRANSACTION_HISTORY: &str = "get_transaction_history";
        }
    }
    
    /// Module Exchange - Quản lý hoạt động trao đổi token
    pub mod exchange {
        /// Version hiện tại của Exchange module
        pub const VERSION: &str = "0.3.0";
        
        /// Mô tả: Exchange module quản lý tất cả các hoạt động trao đổi token,
        /// bao gồm swap, cung cấp thanh khoản, và quản lý các cặp token.
        pub struct ExchangeManifest;
        
        /// Các tính năng chính của Exchange module
        pub mod features {
            /// Swap token
            pub const SWAP_TOKENS: &str = "swap_tokens";
            
            /// Cung cấp thanh khoản
            pub const PROVIDE_LIQUIDITY: &str = "provide_liquidity";
            
            /// Rút thanh khoản
            pub const WITHDRAW_LIQUIDITY: &str = "withdraw_liquidity";
            
            /// Tính giá token
            pub const CALCULATE_PRICE: &str = "calculate_price";
            
            /// Tính slippage
            pub const CALCULATE_SLIPPAGE: &str = "calculate_slippage";
            
            /// Quản lý cặp token
            pub const MANAGE_PAIRS: &str = "manage_pairs";
        }
    }
    
    /// Module SmartContracts - Tương tác với các smart contract và ví
    pub mod smart_contracts {
        /// Version hiện tại của SmartContracts module
        pub const VERSION: &str = "0.2.5";
        
        /// Mô tả: SmartContracts module quản lý tất cả các tương tác với smart contracts và các thao tác liên quan đến ví (wallet): tạo ví từ private key, ký giao dịch, kiểm tra số dư, chuyển token, bridge token... Các module farm/stake cần tích hợp qua tầng service hoặc gọi provider từ smartcontracts khi cần thao tác với ví. 
        /// 
        /// **Chỉ mục liên kết:**
        /// - Các hàm thao tác ví (tạo ví, ký giao dịch, kiểm tra số dư, chuyển token, bridge...) được định nghĩa tại smartcontracts và liên kết trực tiếp đến wallet/src. Khi cần tra cứu hoặc mở rộng các thao tác ví, hãy tham khảo wallet/src để đảm bảo đồng bộ và bảo mật.
        pub struct SmartContractsManifest;
        
        /// Các tính năng chính của SmartContracts module
        pub mod features {
            /// Deploy smart contract
            pub const DEPLOY_CONTRACT: &str = "deploy_contract";
            
            /// Gọi phương thức contract
            pub const CALL_CONTRACT_METHOD: &str = "call_contract_method";
            
            /// Quản lý trạng thái contract
            pub const MANAGE_CONTRACT_STATE: &str = "manage_contract_state";
            
            /// Tương tác với TokenInterface
            pub const INTERACT_WITH_TOKEN_INTERFACE: &str = "interact_with_token_interface";
            
            /// Tương tác với BridgeInterface
            pub const INTERACT_WITH_BRIDGE_INTERFACE: &str = "interact_with_bridge_interface";
        }
    }
    
    // Thêm module common
    pub mod common {
        /// Phiên bản hiện tại của module common
        pub const VERSION: &str = "0.2.0";
        
        /// Thông tin về module common
        pub const DESCRIPTION: &str = "Module chứa các thành phần dùng chung cho các module khác";
        
        /// Manifest cho common module
        pub struct CommonManifest;
        
        /// Tính năng của common module
        pub mod features {
            /// Phát hiện giao dịch đáng ngờ
            pub const SUSPICIOUS_DETECTION: &str = "suspicious_detection";
            
            /// Quản lý cache
            pub const CACHE_MANAGEMENT: &str = "cache_management";
            
            /// Bridge utilities
            pub const BRIDGE_UTILS: &str = "bridge_utils";
            
            /// Farm base
            pub const FARM_BASE: &str = "farm_base";
            
            /// Chain types
            pub const CHAIN_TYPES: &str = "chain_types";
        }
    }
}

/// Mô tả các dịch vụ của hệ thống
pub mod services {
    /// Dịch vụ Transaction - Quản lý giao dịch blockchain
    pub mod transaction {
        /// Version hiện tại của Transaction service
        pub const VERSION: &str = "0.2.0";
        
        /// Mô tả: Transaction service quản lý tất cả các giao dịch blockchain,
        /// bao gồm tạo, ký, và gửi giao dịch đến các blockchain khác nhau.
        pub struct TransactionManifest;
    }
    
    /// Dịch vụ Security - Quản lý bảo mật
    pub mod security {
        /// Version hiện tại của Security service
        pub const VERSION: &str = "0.3.1";
        
        /// Mô tả: Security service quản lý bảo mật cho hệ thống,
        /// bao gồm mã hóa, xác thực, và phát hiện gian lận.
        pub struct SecurityManifest;
    }
    
    /// Dịch vụ Persistence - Quản lý dữ liệu
    pub mod persistence {
        /// Version hiện tại của Persistence service
        pub const VERSION: &str = "0.2.5";
        
        /// Mô tả: Persistence service quản lý dữ liệu cho hệ thống,
        /// bao gồm lưu trữ, truy vấn, và đồng bộ hóa dữ liệu.
        pub struct PersistenceManifest;
    }
    
    /// Dịch vụ Analytics - Phân tích dữ liệu
    pub mod analytics {
        /// Version hiện tại của Analytics service
        pub const VERSION: &str = "0.1.5";
        
        /// Mô tả: Analytics service phân tích dữ liệu của hệ thống,
        /// bao gồm phân tích giao dịch, phát hiện gian lận, và báo cáo.
        pub struct AnalyticsManifest;
    }
}

/// Version chung cho toàn bộ blockchain module
pub const VERSION: &str = "0.5.3"; // Tăng phiên bản từ 0.5.2 lên 0.5.3

/// Mô tả các dependencies của blockchain module
pub mod dependencies {
    /// Thư viện mã hóa
    pub const CRYPTO: &str = "ring:0.16.20";
    
    /// Thư viện serialization/deserialization
    pub const SERDE: &str = "serde:1.0.152";
    
    /// Thư viện tương tác với Web3
    pub const WEB3: &str = "web3:0.18.0";
    
    /// SDK cho NEAR Protocol
    pub const NEAR_SDK: &str = "near-sdk:4.1.1";
    
    /// Thư viện logging
    pub const LOG: &str = "log:0.4.17";
    
    /// Thư viện async runtime
    pub const TOKIO: &str = "tokio:1.25.0";
    
    /// Thư viện error handling
    pub const ANYHOW: &str = "anyhow:1.0.69";
    
    /// Thư viện JSON
    pub const SERDE_JSON: &str = "serde_json:1.0.93";
}

/// Mô tả các tính năng được lên kế hoạch
pub mod planned_features {
    /// Tính năng dự kiến cho Bridge module
    pub mod bridge {
        /// Hỗ trợ blockchain mới
        pub const NEW_BLOCKCHAIN_SUPPORT: &str = "planned:0.6.0";
        
        /// Hệ thống giám sát
        pub const MONITORING_SYSTEM: &str = "planned:0.6.0";
        
        /// Hỗ trợ tiêu chuẩn token mới
        pub const NEW_TOKEN_STANDARDS: &str = "planned:0.7.0";
        
        /// Cải thiện phát hiện giao dịch đáng ngờ
        pub const ENHANCED_SUSPICIOUS_DETECTION: &str = "completed:0.3.5";
    }
    
    /// Tính năng dự kiến cho Oracle module
    pub mod oracle {
        /// Dashboard cho Oracle
        pub const ORACLE_DASHBOARD: &str = "planned:0.6.0";
        
        /// Backup và recovery
        pub const BACKUP_RECOVERY: &str = "planned:0.6.0";
        
        /// Dự đoán giá
        pub const PRICE_PREDICTION: &str = "planned:0.7.0";
        
        /// Cải thiện phát hiện giao dịch bất thường
        pub const ENHANCED_ABNORMAL_DETECTION: &str = "completed:0.2.8";
    }
    
    /// Tính năng dự kiến cho Exchange module
    pub mod exchange {
        /// Lưu trữ persistent cho cặp token
        pub const PERSISTENT_PAIRS: &str = "planned:0.5.0";
        
        /// Cải thiện phí giao dịch
        pub const IMPROVED_FEES: &str = "planned:0.5.0";
        
        /// Bảo vệ khỏi thao túng giá
        pub const PRICE_MANIPULATION_PROTECTION: &str = "planned:0.6.0";
    }
    
    pub mod common {
        /// Module phát hiện giao dịch đáng ngờ chung
        pub const SUSPICIOUS_DETECTION_MODULE: &str = "completed:0.2.0";
        
        /// Cải thiện cache management
        pub const ENHANCED_CACHE_MANAGEMENT: &str = "planned:0.2.5";
    }
}

/// Mô tả các API của blockchain module
pub mod apis {
    /// API cho Bridge module
    pub mod bridge_api {
        /// Version hiện tại của Bridge API
        pub const VERSION: &str = "0.3.0";
        
        /// Path cơ sở cho Bridge API
        pub const BASE_PATH: &str = "/api/v1/bridge";
        
        /// Các endpoint của Bridge API
        pub mod endpoints {
            /// Tạo giao dịch bridge
            pub const CREATE_TRANSACTION: &str = "/transaction";
            
            /// Lấy trạng thái giao dịch bridge
            pub const GET_TRANSACTION_STATUS: &str = "/transaction/:id/status";
            
            /// Liệt kê các giao dịch bridge
            pub const LIST_TRANSACTIONS: &str = "/transactions";
            
            /// Lấy danh sách adapter được hỗ trợ
            pub const GET_SUPPORTED_ADAPTERS: &str = "/adapters";
            
            /// Xác thực giao dịch bridge
            pub const VALIDATE_TRANSACTION: &str = "/transaction/validate";
            
            /// Kiểm tra giao dịch bất thường
            pub const CHECK_ABNORMAL_TRANSACTION: &str = "/transaction/check-abnormal";
        }
    }
    
    /// API cho Oracle module
    pub mod oracle_api {
        /// Version hiện tại của Oracle API
        pub const VERSION: &str = "0.2.0";
        
        /// Path cơ sở cho Oracle API
        pub const BASE_PATH: &str = "/api/v1/oracle";
        
        /// Các endpoint của Oracle API
        pub mod endpoints {
            /// Lấy giá token
            pub const GET_TOKEN_PRICE: &str = "/price/:token";
            
            /// Liệt kê các provider dữ liệu
            pub const LIST_PROVIDERS: &str = "/providers";
            
            /// Thêm provider dữ liệu
            pub const ADD_PROVIDER: &str = "/provider";
            
            /// Xác thực giá token
            pub const VERIFY_PRICE: &str = "/price/verify";
            
            /// Phát hiện giao dịch bất thường
            pub const DETECT_ABNORMAL: &str = "/transaction/detect-abnormal";
            
            /// Xác thực chéo giao dịch
            pub const CROSS_VALIDATE: &str = "/transaction/cross-validate";
        }
    }
}
