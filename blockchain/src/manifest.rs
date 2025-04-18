// File: blockchain/src/manifest.rs
// DiamondChain Blockchain Module Manifest
// 
// Văn bản này mô tả các thành phần và dịch vụ có trong blockchain module,
// bao gồm các chức năng chính và version hiện tại của mỗi thành phần.

/// Version chung cho toàn bộ blockchain module
pub const VERSION: &str = "0.5.1"; // Cập nhật version sau các cải tiến lớn về Bridge và Oracle

/// Mô tả các thành phần của hệ thống
pub mod components {
    /// Module Bridge - Quản lý chuyển token giữa các blockchain khác nhau
    pub mod bridge {
        /// Version hiện tại của Bridge module
        pub const VERSION: &str = "0.3.3";
        
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
        }
    }
    
    /// Module Oracle - Cung cấp dữ liệu giá và thông tin từ các nguồn tin cậy
    pub mod oracle {
        /// Version hiện tại của Oracle module
        pub const VERSION: &str = "0.2.6";
        
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
        }
    }
    
    /// Module Wallet - Quản lý ví và tương tác với blockchain
    pub mod wallet {
        /// Version hiện tại của Wallet module
        pub const VERSION: &str = "0.4.1";
        
        /// Mô tả: Wallet module quản lý tất cả các tương tác với blockchain,
        /// bao gồm tạo và quản lý ví, ký giao dịch, và tương tác với smart contracts.
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
    
    /// Module SmartContracts - Tương tác với các smart contract
    pub mod smart_contracts {
        /// Version hiện tại của SmartContracts module
        pub const VERSION: &str = "0.2.5";
        
        /// Mô tả: SmartContracts module quản lý tất cả các tương tác với smart contracts,
        /// bao gồm deployment, gọi phương thức, và quản lý trạng thái.
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
        pub const VERSION: &str = "0.3.0";
        
        /// Mô tả: Security service quản lý tất cả các khía cạnh bảo mật,
        /// bao gồm mã hóa, quản lý khóa, và xác thực.
        /// 
        /// Cập nhật mới:
        /// - Cải thiện quản lý khóa bí mật cho BridgeConfig
        /// - Thêm cơ chế phát hiện giao dịch bất thường để ngăn chặn tấn công
        /// - Cải thiện xác thực cho các giao dịch bridge
        pub struct SecurityManifest;
    }
    
    /// Dịch vụ Persistence - Quản lý lưu trữ dữ liệu
    pub mod persistence {
        /// Version hiện tại của Persistence service
        pub const VERSION: &str = "0.2.5";
        
        /// Mô tả: Persistence service quản lý lưu trữ dữ liệu,
        /// bao gồm lưu trữ transactional data và application state.
        /// 
        /// Cập nhật mới:
        /// - Thêm persistent storage cho Bridge Transactions sử dụng JSON file
        /// - Cải thiện quản lý cache và cơ chế dọn dẹp tự động
        /// - Thêm các phương thức đồng bộ hóa dữ liệu giữa memory cache và persistent storage
        pub struct PersistenceManifest;
    }
    
    /// Dịch vụ Analytics - Phân tích dữ liệu
    pub mod analytics {
        /// Version hiện tại của Analytics service
        pub const VERSION: &str = "0.1.5";
        
        /// Mô tả: Analytics service phân tích dữ liệu blockchain và token,
        /// cung cấp thông tin về xu hướng thị trường và hiệu suất token.
        pub struct AnalyticsManifest;
    }
}

/// Các dependency/thư viện chính được sử dụng
pub mod dependencies {
    /// Thư viện cryptography
    pub const CRYPTO: &str = "ring:0.16.20";
    
    /// Thư viện serialization
    pub const SERDE: &str = "serde:1.0.152";
    
    /// Thư viện blockchain
    pub const WEB3: &str = "web3:0.18.0";
    
    /// Thư viện NEAR Protocol
    pub const NEAR_SDK: &str = "near-sdk:4.1.1";
    
    /// Thư viện logging
    pub const LOG: &str = "log:0.4.17";
    
    /// Thư viện async runtime
    pub const TOKIO: &str = "tokio:1.25.0";
    
    /// Thư viện error handling
    pub const ANYHOW: &str = "anyhow:1.0.69";
    
    /// Thư viện JSON processing
    pub const SERDE_JSON: &str = "serde_json:1.0.93";
}

/// Thông tin về các tính năng đã lập kế hoạch cho phiên bản tương lai
pub mod planned_features {
    /// Các tính năng đã lên kế hoạch cho module Bridge
    pub mod bridge {
        /// Hỗ trợ các blockchain mới (Solana, Avalanche, Cosmos)
        pub const NEW_BLOCKCHAIN_SUPPORT: &str = "planned:0.6.0";
        
        /// Cải thiện monitoring và alerting system
        pub const MONITORING_SYSTEM: &str = "planned:0.6.0";
        
        /// Hỗ trợ token standards mới (ERC721, etc)
        pub const NEW_TOKEN_STANDARDS: &str = "planned:0.7.0";
    }
    
    /// Các tính năng đã lên kế hoạch cho module Oracle
    pub mod oracle {
        /// Dashboard cho theo dõi trạng thái Oracle
        pub const ORACLE_DASHBOARD: &str = "planned:0.6.0";
        
        /// Cơ chế backup và recovery cho Oracle data
        pub const BACKUP_RECOVERY: &str = "planned:0.6.0";
        
        /// Phân tích thông minh để dự đoán giá token
        pub const PRICE_PREDICTION: &str = "planned:0.7.0";
    }
    
    /// Các tính năng đã lên kế hoạch cho module Exchange
    pub mod exchange {
        /// Persistent storage cho pairs data
        pub const PERSISTENT_PAIRS: &str = "planned:0.5.0";
        
        /// Cải thiện fee handling
        pub const IMPROVED_FEES: &str = "planned:0.5.0";
        
        /// Cơ chế chống price manipulation
        pub const PRICE_MANIPULATION_PROTECTION: &str = "planned:0.6.0";
    }
}

/// Thông tin về các API của hệ thống
pub mod apis {
    /// API cho Bridge module
    pub mod bridge_api {
        /// Version hiện tại của Bridge API
        pub const VERSION: &str = "0.3.0";
        
        /// Base path cho Bridge API
        pub const BASE_PATH: &str = "/api/v1/bridge";
        
        /// Các endpoints của Bridge API
        pub mod endpoints {
            /// Endpoint để tạo bridge transaction
            pub const CREATE_TRANSACTION: &str = "/transaction";
            
            /// Endpoint để lấy trạng thái transaction
            pub const GET_TRANSACTION_STATUS: &str = "/transaction/:id/status";
            
            /// Endpoint để lấy danh sách transaction
            pub const LIST_TRANSACTIONS: &str = "/transactions";
            
            /// Endpoint để lấy thông tin về các adapter được hỗ trợ
            pub const GET_SUPPORTED_ADAPTERS: &str = "/adapters";
            
            /// Endpoint để xác thực giao dịch
            pub const VALIDATE_TRANSACTION: &str = "/transaction/validate";
            
            /// Endpoint để kiểm tra giao dịch bất thường
            pub const CHECK_ABNORMAL_TRANSACTION: &str = "/transaction/check-abnormal";
        }
    }
    
    /// API cho Oracle module
    pub mod oracle_api {
        /// Version hiện tại của Oracle API
        pub const VERSION: &str = "0.2.0";
        
        /// Base path cho Oracle API
        pub const BASE_PATH: &str = "/api/v1/oracle";
        
        /// Các endpoints của Oracle API
        pub mod endpoints {
            /// Endpoint để lấy giá token
            pub const GET_TOKEN_PRICE: &str = "/price/:token";
            
            /// Endpoint để lấy danh sách các provider
            pub const LIST_PROVIDERS: &str = "/providers";
            
            /// Endpoint để thêm provider mới
            pub const ADD_PROVIDER: &str = "/provider";
            
            /// Endpoint để xác thực giá consensus
            pub const VERIFY_PRICE: &str = "/price/verify";
            
            /// Endpoint để kiểm tra giao dịch bất thường
            pub const DETECT_ABNORMAL: &str = "/transaction/detect-abnormal";
            
            /// Endpoint để cross-validate giao dịch
            pub const CROSS_VALIDATE: &str = "/transaction/cross-validate";
        }
    }
} 