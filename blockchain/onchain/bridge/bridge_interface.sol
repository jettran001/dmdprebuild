// SPDX-License-Identifier: MIT
pragma solidity 0.8.20;

import "@layerzerolabs/solidity-examples/contracts/lzApp/NonblockingLzApp.sol";
import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import "@openzeppelin/contracts/security/Pausable.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/security/ReentrancyGuard.sol";
import "@openzeppelin/contracts/token/ERC1155/IERC1155.sol";
import "@openzeppelin/contracts/utils/Address.sol";
import "@openzeppelin/contracts/utils/cryptography/ECDSA.sol";
<<<<<<< HEAD
import "./libraries/ChainRegistry.sol";
import "./libraries/BridgePayloadCodec.sol";
=======
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39

/**
 * @title IWrappedDMD
 * @dev Interface cho WrappedDMD token
 */
interface IWrappedDMD {
    function mint(address to, uint256 amount) external;
    function burnFrom(address account, uint256 amount) external;
    function balanceOf(address account) external view returns (uint256);
    function allowance(address owner, address spender) external view returns (uint256);
    function verifyProxyOperation(uint8 operation) external view returns (bool);
}

/**
 * @title IDiamondToken
 * @dev Interface cho DMD token ERC-1155
 */
interface IDiamondToken {
    function safeTransferFrom(address from, address to, uint256 id, uint256 amount, bytes calldata data) external;
    function balanceOf(address account, uint256 id) external view returns (uint256);
}

/**
 * @title IErc1155Wrapper
 * @dev Interface cho ERC1155Wrapper 
 */
interface IErc1155Wrapper {
    function unwrap(uint256 amount) external;
    function wrap(uint256 amount) external;
    function wrapAndBridgeWithUnwrap(uint256 amount, uint16 dstChainId, bytes memory toAddress) external payable;
}

/**
 * @title IBridgeCoordinator
 * @dev Interface cho BridgeCoordinator - dùng để điều phối các contracts bridge
 */
interface IBridgeCoordinator {
    function recordBridgeOperation(
        bytes32 bridgeId,
        address sender,
        address token,
        uint16 dstChainId,
        bytes memory toAddress,
        uint256 amount,
        string memory status
    ) external;
    
    function updateBridgeStatus(bytes32 bridgeId, string memory status) external;
    
    function getBridgeOperation(bytes32 bridgeId) external view returns (
        address sender,
        address token,
        uint16 dstChainId,
        bytes memory toAddress,
        uint256 amount,
        uint256 timestamp,
        bool completed,
        string memory status
    );
    
    function retryFailedOperation(bytes32 bridgeId) external payable;
}

/**
 * @title IERC721
 * @dev Interface for ERC721 token
 */
interface IERC721 {
    function safeTransferFrom(address from, address to, uint256 tokenId, bytes calldata data) external;
    function ownerOf(uint256 tokenId) external view returns (address);
    function isApprovedForAll(address owner, address operator) external view returns (bool);
    function approve(address to, uint256 tokenId) external;
    function getApproved(uint256 tokenId) external view returns (address);
}

/**
 * @title IERC777
 * @dev Interface for ERC777 token
 */
interface IERC777 {
    function operatorSend(address sender, address recipient, uint256 amount, bytes calldata data, bytes calldata operatorData) external;
    function balanceOf(address holder) external view returns (uint256);
    function isOperatorFor(address operator, address tokenHolder) external view returns (bool);
    function authorizeOperator(address operator) external;
    function revokeOperator(address operator) external;
}

/**
 * @title DiamondBridgeInterface
 * @dev Contract for bridging DMD tokens between different blockchains using LayerZero
 */
contract DiamondBridgeInterface is NonblockingLzApp, Ownable, Pausable, ReentrancyGuard {
    using SafeERC20 for IERC20;
    using Address for address;
    
    // Custom Errors
    error ZeroAddress();
    error EmptyDestination();
    error InvalidAmount();
    error ChainNotSupported();
    error TrustedRemoteNotSet();
    error InvalidSourceAddress();
    error InvalidSourceLength();
    error FeeExceedsAmount();
    error FeeCollectorNotSet();
    error InvalidDestinationAddress();
    error InsufficientAllowance();
    error TimelockNotExpired();
    error InvalidTimestamp();
    error NonceAlreadyProcessed();
    error ExceededRateLimit();
    error MintFailed(string reason);
    error UnsupportedToken();
    error InsufficientGasFee();
    error InvalidGasAmount();
    error TokenTransferFailed();
    error InvalidTrustedRemote();
    error InvalidSourceChainId();
    error InvalidFeeBps();
    error InvalidFeeParameters();
    error ZeroAddressNotAllowed();
    error InvalidMessageLength();
    error NotAuthorized();
    error BridgeOperationNotFound();
    error CoordinatorNotSet();
    error UnwrapperNotRegistered(uint16 chainId);
    error UnwrapperNotContract(address unwrapper);
    error UnwrapFailed(address token, address unwrapper, uint256 amount);
<<<<<<< HEAD
    error ConfirmationAlreadyProcessed();
    error ConfirmationExpired();
    error InvalidSignature();
    error InvalidStateRoot();
    
    // Constants for message types
    uint8 constant private MESSAGE_TYPE_BRIDGE = 1;
    uint8 constant private MESSAGE_TYPE_CONFIRMATION = 2;
    uint8 constant private MESSAGE_TYPE_EMERGENCY = 3;
    uint256 constant private CONFIRMATION_EXPIRY = 24 hours;
    
    // TransactionState structure for cross-chain state management
    struct TransactionState {
        bytes32 txId;                
        uint8 status;                
        uint256 timestamp;           
        bytes signature;             
        address validator;           
        bytes32 stateRoot;           
        uint16 sourceChain;          
        uint16 destinationChain;     
        address sender;              
        address recipient;           
        uint256 amount;              
        string errorMessage;         
    }
    
    // Mappings for transaction state management
    mapping(bytes32 => TransactionState) public transactionStates;
    mapping(bytes32 => bool) public processedConfirmations;
    mapping(uint16 => mapping(address => uint64)) public nextNonce; 
    
    // Events for tracking state changes
    event TransactionStateChanged(
        bytes32 indexed txId, 
        uint8 status, 
        uint256 timestamp,
        address indexed validator
    );
    
    event ConfirmationSent(
        bytes32 indexed txId, 
        uint16 srcChainId, 
        uint16 destChainId,
        uint8 status,
        uint256 timestamp
    );
    
    event ConfirmationReceived(
        bytes32 indexed txId, 
        uint16 srcChainId, 
        uint8 status,
        uint256 timestamp
    );
    
    event RecoveryInitiated(
        bytes32 indexed txId,
        address indexed executor,
        uint256 timestamp
    );
    
    // BridgeOperation structure
    struct BridgeOperation {
        address sender;           
        address token;            
        uint16 destinationChainId; 
        bytes destinationAddress;  
        uint256 amount;           
        uint256 timestamp;        
        bool completed;           
        string status;            
        bytes32 relatedOpId;      
    }
    
    // Storage for bridge operations
    mapping(bytes32 => BridgeOperation) public bridgeOperations;
    mapping(address => bytes32[]) public userBridgeOperations;
    bytes32[] public allBridgeOperations;
    uint256 public constant MAX_OPERATIONS = 10000;
    uint256 public constant MAX_STORAGE_TIME = 30 days;
    IBridgeCoordinator public bridgeCoordinator;
    bool public useCoordinator = false;
    
    // Events
=======
    
    // -------------- BUG #7 FIX: Bridge Operation Tracking ----------------
    // Cấu trúc BridgeOperation để theo dõi các hoạt động bridge
    struct BridgeOperation {
        address sender;           // Địa chỉ người gửi
        address token;            // Địa chỉ token được bridge
        uint16 destinationChainId; // Chain ID đích
        bytes destinationAddress;  // Địa chỉ đích
        uint256 amount;           // Số lượng token
        uint256 timestamp;        // Thời gian tạo
        bool completed;           // Trạng thái hoàn thành
        string status;            // Trạng thái chi tiết (Success/Failed/Pending)
        bytes32 relatedOpId;      // Bridge ID liên quan (nếu có)
    }
    
    // Lưu trữ các bridge operation
    mapping(bytes32 => BridgeOperation) public bridgeOperations;
    
    // Lưu trữ danh sách bridge operation theo địa chỉ
    mapping(address => bytes32[]) public userBridgeOperations;
    
    // Danh sách tất cả bridge operation
    bytes32[] public allBridgeOperations;
    
    // BridgeCoordinator address - Trung tâm điều phối các hoạt động bridge
    IBridgeCoordinator public bridgeCoordinator;
    
    // Flag đánh dấu có sử dụng coordinator hay không
    bool public useCoordinator = false;
    
    // -------------- END BUG #7 FIX ----------------
    
    // Events
    // Cập nhật event với bridge ID
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    event BridgeInitiated(
        bytes32 indexed bridgeId,
        address indexed sender,
        address indexed token,
        uint256 amount,
        uint16 destinationChainId,
        bytes destinationAddress,
        uint256 fee,
        uint256 timestamp
    );
    
    event BridgeReceived(
        bytes32 indexed bridgeId,
        address indexed sender,
        address indexed recipient,
        uint256 amount,
        uint16 srcChainId,
        uint64 nonce,
        uint256 timestamp
    );
    
    event BridgeStatusUpdated(
        bytes32 indexed bridgeId,
        string status,
        uint256 timestamp
    );
    
<<<<<<< HEAD
=======
    // Fix bug #13: Chuẩn hóa thêm các event mới để đồng bộ giữa các contracts
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    event BridgeOperationCreated(
        bytes32 indexed bridgeId,
        address indexed sender,
        address indexed token,
        uint16 destinationChainId,
        bytes destinationAddress,
        uint256 amount,
        string status,
        uint256 timestamp
    );
    
    event BridgeOperationUpdated(
        bytes32 indexed bridgeId,
        string status,
        bool completed,
        uint256 timestamp
    );
    
    event TokenWrapped(
        bytes32 indexed bridgeId,
        address indexed sender,
        address indexed token,
        uint256 amount,
        uint256 timestamp
    );
    
    event TokenUnwrapped(
        bytes32 indexed bridgeId,
        address indexed sender,
        address indexed token,
        uint256 amount,
        uint256 timestamp
    );
    
    event AutoUnwrapProcessed(
        bytes32 indexed bridgeId,
        address indexed user,
        address indexed unwrapper,
        uint256 amount,
        bool success,
        uint256 timestamp
    );
    
    event FailedTransactionStored(
        uint16 indexed srcChainId,
        address indexed to,
        uint256 amount,
        uint64 nonce,
        string reason,
        uint256 timestamp
    );
    
    event RetryFailed(
        bytes32 indexed bridgeId,
        string reason,
        uint256 timestamp
    );
    
<<<<<<< HEAD
    event FeeUpdated(
        uint256 baseFee, 
        uint256 feePercentage,
        address indexed updater,
        uint256 timestamp
    );
    
    event FeeCollectorUpdated(
        address indexed oldCollector, 
        address indexed newCollector,
        address indexed updater,
        uint256 timestamp
    );
    
    event TrustedRemoteUpdateRequested(
        uint16 indexed chainId, 
        bytes remoteAddress, 
        uint256 unlockTime,
        address indexed requester,
        uint256 timestamp
    );
    
    event TrustedRemoteUpdated(
        uint16 indexed chainId, 
        bytes remoteAddress,
        address indexed updater,
        uint256 timestamp
    );
    
    event TrustedRemoteUpdateCanceled(
        uint16 indexed chainId,
        address indexed canceler,
        uint256 timestamp
    );
    
    event TokenAddressUpdated(
        address indexed oldToken, 
        address indexed newToken,
        uint16 indexed chainId,
        address updater,
        uint256 timestamp
    );
    
    event TokenBlacklisted(
        address indexed token, 
        bool blacklisted,
        address indexed operator,
        uint256 timestamp
    );
    
    event TokenWhitelisted(
        address indexed token, 
        bool whitelisted,
        address indexed operator,
        uint256 timestamp
    );
    
    event DailyLimitUpdated(
        uint256 oldLimit,
        uint256 newLimit,
        address indexed updater,
        uint256 timestamp
    );
    
    event TransactionLimitUpdated(
        uint256 oldLimit,
        uint256 newLimit,
        address indexed updater,
        uint256 timestamp
    );
    
    event FailedMint(
        bytes32 indexed bridgeId,
        uint16 indexed srcChainId, 
        address indexed to, 
        uint256 amount, 
        string reason,
        uint256 timestamp
    );
    
    event TokenTypeSupported(
        address indexed token, 
        uint8 tokenType,
        address indexed operator,
        uint256 timestamp
    );
    
    event DefaultFeesUpdated(
        uint16 indexed chainId,
        uint256 oldBaseFee,
        uint256 newBaseFee, 
        uint256 oldFeePercentage,
        uint256 newFeePercentage,
        address indexed updater,
        uint256 timestamp
    );
    
    event CoordinatorUpdated(
        address indexed oldCoordinator, 
        address indexed newCoordinator,
        address indexed updater,
        uint256 timestamp
    );
    
    event CoordinatorStatusChanged(
        bool oldStatus,
        bool newStatus,
        address indexed updater,
        uint256 timestamp
    );
    
    event RetryOperation(
        bytes32 indexed bridgeId, 
        string operationType,
        address indexed retrier,
        uint256 timestamp
    );
    
    event FailedTransactionProcessed(
        bytes32 indexed txId,
        address indexed processor,
        uint256 timestamp
    );
    
    event ProcessedFailedTransactionsCleared(
        uint256 count,
        address indexed clearer,
        uint256 timestamp
    );
    
    event FailedTransactionLimitReached(
        bytes32 oldestTxIdRemoved, 
        bytes32 newTxIdAdded,
        uint256 limit,
        uint256 timestamp
    );
    
    event UnwrapperRegistered(
        uint16 indexed chainId, 
        address indexed unwrapper,
        address indexed registrar,
        uint256 timestamp
    );
    
    event UnwrapperRemoved(
        uint16 indexed chainId, 
        address indexed unwrapper,
        address indexed remover,
        uint256 timestamp
    );
    
    event BridgeOperationsCleaned(
        uint256 count, 
        uint256 threshold,
        address indexed cleaner,
        uint256 timestamp
    );
    
    event StuckTokenRescued(
        address indexed token, 
        address indexed to, 
        uint256 amount,
        address indexed rescuer,
        uint256 timestamp
    );
    
    event StuckERC1155Rescued(
        address indexed token, 
        address indexed to, 
        uint256 tokenId, 
        uint256 amount,
        address indexed rescuer,
        uint256 timestamp
    );
    
    event StuckETHRescued(
        address indexed to, 
        uint256 amount,
        address indexed rescuer,
        uint256 timestamp
    );
    
    // Variables
    address public wrappedDMDToken;
    uint256 public feePercentage = 0; 
    uint256 public constant MAX_FEE = 100; 
    address public feeCollector;
    // Thay thế các mapping liên quan đến chainId bằng registry chuẩn hóa
    ChainRegistry.Registry private chainRegistry;
    // mapping(uint16 => bytes) public trustedRemotes; // XÓA
    // mapping(uint16 => address) public wrappedTokenByChain; // XÓA
    // mapping(uint16 => address) public unwrapperRegistry; // XÓA
    
    uint256 public constant DMD_TOKEN_ID = 0;
    uint256 public constant TIMELOCK_DURATION = 2 days;
    uint256 public constant EMERGENCY_TIMELOCK = 1 days;
    
=======
    event FeeUpdated(uint256 baseFee, uint256 feePercentage);
    event FeeCollectorUpdated(address indexed newCollector);
    event TrustedRemoteUpdateRequested(uint16 indexed chainId, bytes remoteAddress, uint256 unlockTime);
    event TrustedRemoteUpdated(uint16 indexed chainId, bytes remoteAddress);
    event TrustedRemoteUpdateCanceled(uint16 indexed chainId);
    event TokenAddressUpdated(address indexed oldToken, address indexed newToken);
    event TokenBlacklisted(address indexed token, bool blacklisted);
    event TokenWhitelisted(address indexed token, bool whitelisted);
    event DailyLimitUpdated(uint256 newLimit);
    event TransactionLimitUpdated(uint256 newLimit);
    event FailedMint(uint16 indexed srcChainId, address indexed to, uint256 amount, string reason);
    event TokenTypeSupported(address indexed token, uint8 tokenType);
    event EmergencyWithdrawal(address indexed token, address indexed to, uint256 amount);
    event DefaultFeesUpdated(uint16 _chainId, uint256 _baseFee, uint256 _feePercentage);
    event CoordinatorUpdated(address indexed oldCoordinator, address indexed newCoordinator);
    event CoordinatorStatusChanged(bool useCoordinator);
    event RetryOperation(bytes32 indexed bridgeId, string operationType);
    event FailedTransactionProcessed(bytes32 indexed txId);
    event ProcessedFailedTransactionsCleared(uint256 count);
    event FailedTransactionLimitReached(bytes32 oldestTxIdRemoved, bytes32 newTxIdAdded);
    event UnwrapperRegistered(uint16 indexed chainId, address indexed unwrapper);
    event UnwrapperRemoved(uint16 indexed chainId, address indexed unwrapper);
    event AutoUnwrapFailed(address indexed token, address indexed recipient, uint256 amount, string reason);
    
    // Variables
    address public wrappedDMDToken;
    uint256 public feePercentage = 0; // 0.1% = 1, 1% = 10
    uint256 public constant MAX_FEE = 100; // tối đa 10%
    address public feeCollector;
    mapping(uint16 => bytes) public trustedRemotes; // chainId => trusted remote address
    
    // DMD token ID cho ERC-1155
    uint256 public constant DMD_TOKEN_ID = 0;
    
    // Timelock cho trusted remote updates
    uint256 public constant TIMELOCK_DURATION = 2 days;
    
    // Timelock cho các hành động khẩn cấp
    uint256 public constant EMERGENCY_TIMELOCK = 1 days;
    
    // Cấu trúc để lưu trữ yêu cầu cập nhật trusted remote
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    struct TrustedRemoteUpdate {
        bytes remoteAddress;
        uint256 unlockTime;
        bool executed;
    }
    
<<<<<<< HEAD
    mapping(uint16 => TrustedRemoteUpdate) public pendingTrustedRemoteUpdates;
    
    // Gom các biến limit, rate, fee về một nơi duy nhất
    struct BridgeConfig {
        uint256 dailyLimit;
        uint256 transactionLimit;
        uint256 dailyUnwrapLimit;
        uint256 baseFee;
        uint256 feePercentage;
        address feeCollector;
    }
    mapping(uint16 => BridgeConfig) public bridgeConfigs; // chainId => config
    
    mapping(address => bool) public whitelistedTokens;
    mapping(address => bool) public blacklistedTokens;
    
=======
    // Lưu trữ các yêu cầu cập nhật trusted remote
    mapping(uint16 => TrustedRemoteUpdate) public pendingTrustedRemoteUpdates;
    
    // Rate limiting
    uint256 public dailyLimit = 1000000 ether; // Giới hạn số lượng token mỗi ngày
    uint256 public transactionLimit = 10000 ether; // Giới hạn số lượng token mỗi giao dịch
    mapping(uint256 => uint256) public dailyVolume; // day => volume
    uint256 public constant EXPIRY_WINDOW = 1 hours; // Thời gian hết hạn cho giao dịch
    
    // Lưu trữ nonce đã xử lý để tránh replay attack
    mapping(uint16 => mapping(uint64 => bool)) public processedNonces;
    
    // Thông tin token bị mắc kẹt
    struct FailedTransaction {
        uint16 srcChainId;
        address to;
        uint256 amount;
        uint64 nonce;
        string reason;
        uint256 timestamp;
        bool processed;
    }
    
    // Lưu trữ thông tin token bị mắc kẹt
    mapping(bytes32 => FailedTransaction) public failedTransactions;
    
    // Danh sách ID của các giao dịch thất bại được lưu trữ
    bytes32[] public failedTransactionIds;
    
    // Giới hạn số lượng giao dịch thất bại được lưu trữ
    uint256 public constant MAX_FAILED_TXS = 1000;
    
    // Whitelisted token types
    mapping(address => bool) public whitelistedTokens;
    
    // Blacklisted token addresses
    mapping(address => bool) public blacklistedTokens;
    
    // Token types
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    uint8 public constant TOKEN_TYPE_ERC20 = 1;
    uint8 public constant TOKEN_TYPE_ERC1155 = 2;
    uint8 public constant TOKEN_TYPE_NATIVE = 3;
    uint8 public constant TOKEN_TYPE_ERC721 = 4;
    uint8 public constant TOKEN_TYPE_ERC777 = 5;
    
<<<<<<< HEAD
    mapping(address => uint8) public tokenTypes;
    // mapping(uint16 => address) public wrappedTokenByChain; // XÓA
    // mapping(uint16 => address) public unwrapperRegistry; // XÓA
    
    struct Fee {
        uint256 baseFee; 
        uint256 feePercentage; 
    }
    
    mapping(uint16 => Fee) public defaultFees; 
    mapping(address => bool) public supportedTokens; 
    
    uint256 public constant MAX_FEE_BPS = 1000; 
    uint256 public constant FEE_DENOMINATOR = 10000; 
    address[] public supportedTokensList; 
    
    // --- Add unwrap rate limit tracking ---
    uint256 public dailyUnwrapLimit = 1000000 ether; // daily unwrap limit
    mapping(uint256 => uint256) public dailyUnwrapVolume; // day => volume
    
    // Thêm mapping để lưu trữ chiều dài payload hợp lệ theo loại message
    mapping(uint8 => uint256) public EXPECTED_LENGTH_BY_TYPE;

    // Thêm version byte vào đầu mỗi payload
    uint8 public constant PAYLOAD_VERSION = 1;
    
    // Mapping lưu nonce đã xử lý cho mỗi chainId (chống replay)
    mapping(uint16 => mapping(uint64 => bool)) public processedNonces;
    
    // Đảm bảo chuẩn hóa rescue, stuck, emergency
    // Đã có: requestEmergencyWithdrawal, executeEmergencyWithdrawal, emergencyWithdrawals, event EmergencyWithdrawal, EMERGENCY_TIMELOCK, onlyOwner, limit, timeout
    // Bổ sung event cho rescue stuck token ERC20/ERC1155/ETH nếu chưa có
    event StuckTokenRescued(address indexed token, address indexed to, uint256 amount, address indexed rescuer, uint256 timestamp);
    event StuckERC1155Rescued(address indexed token, address indexed to, uint256 tokenId, uint256 amount, address indexed rescuer, uint256 timestamp);
    event StuckETHRescued(address indexed to, uint256 amount, address indexed rescuer, uint256 timestamp);

    /**
     * @notice Rescue stuck ERC20 tokens from the contract. Only callable by the owner.
     * @param token The ERC20 token address to rescue
     * @param amount The amount of tokens to rescue
     */
    function rescueStuckERC20(address token, uint256 amount) external onlyOwner whenNotPaused {
        require(token != address(0), "Zero address");
        require(amount > 0, "Invalid amount");
        IERC20(token).transfer(owner(), amount);
        emit StuckTokenRescued(token, owner(), amount, msg.sender, block.timestamp);
    }
    /**
     * @notice Rescue stuck ERC1155 tokens from the contract. Only callable by the owner.
     * @param token The ERC1155 token address to rescue
     * @param tokenId The tokenId to rescue
     * @param amount The amount of tokens to rescue
     */
    function rescueStuckERC1155(address token, uint256 tokenId, uint256 amount) external onlyOwner whenNotPaused {
        require(token != address(0), "Zero address");
        require(amount > 0, "Invalid amount");
        IERC1155(token).safeTransferFrom(address(this), owner(), tokenId, amount, "");
        emit StuckERC1155Rescued(token, owner(), tokenId, amount, msg.sender, block.timestamp);
    }
    /**
     * @notice Rescue stuck ETH from the contract. Only callable by the owner.
     * @param amount The amount of ETH to rescue
     */
    function rescueStuckETH(uint256 amount) external onlyOwner whenNotPaused {
        require(amount > 0, "Invalid amount");
        (bool success, ) = owner().call{value: amount}("");
        require(success, "ETH rescue failed");
        emit StuckETHRescued(owner(), amount, msg.sender, block.timestamp);
    }
=======
    // Token type mapping
    mapping(address => uint8) public tokenTypes;
    
    // Different wrapper addresses by chain
    mapping(uint16 => address) public wrappedTokenByChain;
    
    // Struct for emergency withdrawal
    struct EmergencyWithdrawalRequest {
        address token;
        address recipient;
        uint256 amount;
        uint256 unlockTime;
        bool executed;
    }
    
    // Storage for emergency withdrawal requests
    mapping(bytes32 => EmergencyWithdrawalRequest) public emergencyWithdrawals;
    
    // Fee structure
    struct Fee {
        uint256 baseFee; // Base fee in wei
        uint256 feePercentage; // Fee percentage in basis points (1/100 of 1%)
    }
    
    // Contract variables
    mapping(uint16 => Fee) public defaultFees; // Chain ID => Default fee
    mapping(address => bool) public supportedTokens; // Supported ERC20 tokens for bridging
    
    uint256 public constant MAX_FEE_BPS = 1000; // Maximum fee: 10%
    uint256 public constant FEE_DENOMINATOR = 10000; // Denominator for fee calculation (basis points)
    address[] public supportedTokensList; // List of supported tokens
    
    // Fix cho bug #17 - Vấn đề bảo mật trong _processAutoUnwrap
    // Thêm mapping cho unwrapper registry
    mapping(uint16 => address) public unwrapperRegistry;
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    
    /**
     * @dev Constructor
     * @param _lzEndpoint The LayerZero endpoint
     * @param _wrappedDMDToken Wrapped DMD token address
     * @param _feeCollector Fee collector address
     */
    constructor(
        address _lzEndpoint,
        address _wrappedDMDToken,
        address _feeCollector
    ) NonblockingLzApp(_lzEndpoint) Ownable(msg.sender) {
        if (_wrappedDMDToken == address(0)) revert ZeroAddress();
        if (_feeCollector == address(0)) revert ZeroAddress();
        
        wrappedDMDToken = _wrappedDMDToken;
        feeCollector = _feeCollector;
        
<<<<<<< HEAD
=======
        // Whitelist default token
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        whitelistedTokens[_wrappedDMDToken] = true;
        tokenTypes[_wrappedDMDToken] = TOKEN_TYPE_ERC20;
        emit TokenWhitelisted(_wrappedDMDToken, true);
        emit TokenTypeSupported(_wrappedDMDToken, TOKEN_TYPE_ERC20);
<<<<<<< HEAD
        
        // Thiết lập chiều dài dự kiến cho từng loại message
        EXPECTED_LENGTH_BY_TYPE[MESSAGE_TYPE_BRIDGE] = 192; // Minimum length for bridge payload
        EXPECTED_LENGTH_BY_TYPE[MESSAGE_TYPE_CONFIRMATION] = 256; // Minimum length for confirmation payload
        EXPECTED_LENGTH_BY_TYPE[MESSAGE_TYPE_EMERGENCY] = 160; // Minimum length for emergency payload
=======
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }
    
    /**
     * @dev Lấy ngày hiện tại (Unix timestamp chia cho số giây trong 1 ngày)
     * @return Ngày hiện tại
     */
    function _getCurrentDay() internal view returns (uint256) {
        return block.timestamp / 1 days;
    }
    
    /**
     * @dev Kiểm tra và cập nhật khối lượng trong ngày
     * @param amount Số lượng token
     * @return Có vượt quá giới hạn không
     */
    function _checkAndUpdateDailyVolume(uint256 amount) internal returns (bool) {
        uint256 currentDay = _getCurrentDay();
        uint256 newVolume = dailyVolume[currentDay] + amount;
        
        if (newVolume > dailyLimit) {
            return false;
        }
        
        dailyVolume[currentDay] = newVolume;
        return true;
    }
    
    /**
     * @dev Yêu cầu cập nhật trusted remote (với timelock)
     * @param _chainId Chain ID
     * @param _remoteAddress Remote address
     */
    function requestTrustedRemoteUpdate(uint16 _chainId, bytes calldata _remoteAddress) external onlyOwner {
        if (_remoteAddress.length == 0) revert EmptyDestination();
<<<<<<< HEAD
        // Sử dụng ChainRegistry để cập nhật remoteAddress
        ChainRegistry.setChain(chainRegistry, _chainId, ChainRegistry.getChainName(chainRegistry, _chainId), _remoteAddress, true);
        emit TrustedRemoteUpdateRequested(_chainId, _remoteAddress, block.timestamp + TIMELOCK_DURATION, msg.sender, block.timestamp);
=======
        
        pendingTrustedRemoteUpdates[_chainId] = TrustedRemoteUpdate({
            remoteAddress: _remoteAddress,
            unlockTime: block.timestamp + TIMELOCK_DURATION,
            executed: false
        });
        
        emit TrustedRemoteUpdateRequested(_chainId, _remoteAddress, block.timestamp + TIMELOCK_DURATION);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }
    
    /**
     * @dev Thực hiện yêu cầu cập nhật trusted remote sau khi hết thời gian khóa
     * @param _chainId Chain ID
     */
    function executeTrustedRemoteUpdate(uint16 _chainId) external onlyOwner {
<<<<<<< HEAD
        // Lấy remoteAddress từ registry
        bytes memory remoteAddress = ChainRegistry.getRemoteAddress(chainRegistry, _chainId);
        if (remoteAddress.length == 0) revert EmptyDestination();
        // Đảm bảo chỉ owner được phép cập nhật
        ChainRegistry.setChain(chainRegistry, _chainId, ChainRegistry.getChainName(chainRegistry, _chainId), remoteAddress, true);
        emit TrustedRemoteUpdated(_chainId, remoteAddress, msg.sender, block.timestamp);
=======
        TrustedRemoteUpdate storage update = pendingTrustedRemoteUpdates[_chainId];
        
        if (update.remoteAddress.length == 0) revert EmptyDestination();
        if (update.executed) revert InvalidSourceAddress();
        if (block.timestamp < update.unlockTime) revert TimelockNotExpired();
        
        trustedRemotes[_chainId] = update.remoteAddress;
        update.executed = true;
        
        emit TrustedRemoteUpdated(_chainId, update.remoteAddress);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }
    
    /**
     * @dev Hủy yêu cầu cập nhật trusted remote
     * @param _chainId Chain ID
     */
    function cancelTrustedRemoteUpdate(uint16 _chainId) external onlyOwner {
<<<<<<< HEAD
        // Đặt lại supported = false
        ChainRegistry.setChain(chainRegistry, _chainId, ChainRegistry.getChainName(chainRegistry, _chainId), "", false);
        emit TrustedRemoteUpdateCanceled(_chainId, msg.sender, block.timestamp);
=======
        TrustedRemoteUpdate storage update = pendingTrustedRemoteUpdates[_chainId];
        
        if (update.remoteAddress.length == 0) revert EmptyDestination();
        if (update.executed) revert InvalidSourceAddress();
        
        delete pendingTrustedRemoteUpdates[_chainId];
        
        emit TrustedRemoteUpdateCanceled(_chainId);
    }
    
    /**
     * @dev Xác thực trusted remote bằng chữ ký
     * @param _chainId Chain ID
     * @param _remoteAddress Remote address
     * @param _signature Chữ ký của owner xác thực địa chỉ
     * @return Có phải trusted remote hợp lệ không
     */
    function verifyTrustedRemote(uint16 _chainId, bytes calldata _remoteAddress, bytes calldata _signature) external view returns (bool) {
        bytes32 messageHash = keccak256(abi.encodePacked(_chainId, _remoteAddress));
        bytes32 ethSignedMessageHash = keccak256(abi.encodePacked("\x19Ethereum Signed Message:\n32", messageHash));
        
        address signer = ECDSA.recover(ethSignedMessageHash, _signature);
        return signer == owner();
    }
    
    /**
     * @dev Lấy trusted remote address cho một chain
     * @param _chainId Chain ID
     * @return Trusted remote address
     */
    function getTrustedRemote(uint16 _chainId) external view returns (bytes memory) {
        return trustedRemotes[_chainId];
    }
    
    /**
     * @dev Kiểm tra xem một chain có trusted remote được cài đặt không
     * @param _chainId Chain ID
     * @return Có trusted remote không
     */
    function hasTrustedRemote(uint16 _chainId) external view returns (bool) {
        return trustedRemotes[_chainId].length > 0;
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }
    
    /**
     * @dev Tính phí bridge
     * @param _amount Số lượng token
     * @return Phí bridge
     */
    function calculateFee(uint256 _amount) public view returns (uint256) {
<<<<<<< HEAD
=======
        // Sử dụng FEE_DENOMINATOR để tính toán chính xác hơn
        // 1 basis point = 0.01%
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        return (_amount * feePercentage) / FEE_DENOMINATOR;
    }
    
    /**
     * @dev Thêm token vào whitelist
     * @param _token Địa chỉ token
     * @param _tokenType Loại token (1: ERC20, 2: ERC1155, 3: Native, 4: ERC721, 5: ERC777)
     */
    function addWhitelistedToken(address _token, uint8 _tokenType) external onlyOwner {
        if (_token == address(0)) revert ZeroAddress();
        if (_tokenType < TOKEN_TYPE_ERC20 || _tokenType > TOKEN_TYPE_ERC777) revert InvalidAmount();
        
        whitelistedTokens[_token] = true;
        tokenTypes[_token] = _tokenType;
        
<<<<<<< HEAD
=======
        // Thêm vào danh sách token hỗ trợ
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        if (!supportedTokens[_token]) {
            supportedTokens[_token] = true;
            supportedTokensList.push(_token);
        }
        
<<<<<<< HEAD
        emit TokenWhitelisted(_token, true, msg.sender, block.timestamp);
        emit TokenTypeSupported(_token, _tokenType, msg.sender, block.timestamp);
=======
        emit TokenWhitelisted(_token, true);
        emit TokenTypeSupported(_token, _tokenType);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }
    
    /**
     * @dev Xóa token khỏi whitelist
     * @param _token Địa chỉ token
     */
    function removeWhitelistedToken(address _token) external onlyOwner {
        if (_token == address(0)) revert ZeroAddress();
        
        whitelistedTokens[_token] = false;
        
<<<<<<< HEAD
        emit TokenWhitelisted(_token, false, msg.sender, block.timestamp);
=======
        emit TokenWhitelisted(_token, false);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }
    
    /**
     * @dev Thêm token vào blacklist
     * @param _token Địa chỉ token
     */
    function blacklistToken(address _token, bool _blacklisted) external onlyOwner {
        if (_token == address(0)) revert ZeroAddress();
        
        blacklistedTokens[_token] = _blacklisted;
        
<<<<<<< HEAD
        emit TokenBlacklisted(_token, _blacklisted, msg.sender, block.timestamp);
=======
        emit TokenBlacklisted(_token, _blacklisted);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }
    
    /**
     * @dev Cập nhật giới hạn giao dịch
     * @param _dailyLimit Giới hạn số lượng token mỗi ngày
     * @param _transactionLimit Giới hạn số lượng token mỗi giao dịch
     */
    function updateLimits(uint256 _dailyLimit, uint256 _transactionLimit) external onlyOwner {
        if (_dailyLimit == 0 || _transactionLimit == 0) revert InvalidAmount();
        if (_transactionLimit > _dailyLimit) revert InvalidAmount();
        
        dailyLimit = _dailyLimit;
        transactionLimit = _transactionLimit;
        
<<<<<<< HEAD
        emit DailyLimitUpdated(dailyLimit, _dailyLimit, msg.sender, block.timestamp);
        emit TransactionLimitUpdated(transactionLimit, _transactionLimit, msg.sender, block.timestamp);
=======
        emit DailyLimitUpdated(_dailyLimit);
        emit TransactionLimitUpdated(_transactionLimit);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }
    
    /**
     * @dev Yêu cầu rút khẩn cấp token hoặc ETH
     * @param _token Địa chỉ token (address(0) cho ETH)
     * @param _recipient Địa chỉ nhận
     * @param _amount Số lượng token/ETH
     */
<<<<<<< HEAD
    function requestEmergencyWithdrawal(address _token, address _recipient, uint256 _amount) external onlyOwner whenNotPaused {
        if (_recipient == address(0)) revert ZeroAddress();
        if (_amount == 0) revert InvalidAmount();
        bytes32 withdrawalId = keccak256(abi.encodePacked(_token, _recipient, _amount, block.timestamp));
=======
    function requestEmergencyWithdrawal(address _token, address _recipient, uint256 _amount) external onlyOwner {
        if (_recipient == address(0)) revert ZeroAddress();
        if (_amount == 0) revert InvalidAmount();
        
        bytes32 withdrawalId = keccak256(abi.encodePacked(_token, _recipient, _amount, block.timestamp));
        
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        emergencyWithdrawals[withdrawalId] = EmergencyWithdrawalRequest({
            token: _token,
            recipient: _recipient,
            amount: _amount,
            unlockTime: block.timestamp + EMERGENCY_TIMELOCK,
            executed: false
        });
    }
    
    /**
     * @dev Thực hiện rút khẩn cấp token hoặc ETH
     * @param withdrawalId ID của yêu cầu rút
     */
<<<<<<< HEAD
    function executeEmergencyWithdrawal(bytes32 withdrawalId) external onlyOwner whenNotPaused {
        EmergencyWithdrawalRequest storage request = emergencyWithdrawals[withdrawalId];
        if (request.recipient == address(0)) revert ZeroAddress();
        if (request.executed) revert NonceAlreadyProcessed();
        if (block.timestamp < request.unlockTime) revert TimelockNotExpired();
        request.executed = true;
        if (request.token == address(0)) {
            (bool success, ) = request.recipient.call{value: request.amount}("");
            if (!success) revert TokenTransferFailed();
        } else {
            IERC20(request.token).safeTransfer(request.recipient, request.amount);
        }
        emit EmergencyWithdrawal(withdrawalId, request.token, request.recipient, request.amount, msg.sender, block.timestamp);
=======
    function executeEmergencyWithdrawal(bytes32 withdrawalId) external onlyOwner {
        EmergencyWithdrawalRequest storage request = emergencyWithdrawals[withdrawalId];
        
        if (request.recipient == address(0)) revert ZeroAddress();
        if (request.executed) revert NonceAlreadyProcessed();
        if (block.timestamp < request.unlockTime) revert TimelockNotExpired();
        
        request.executed = true;
        
        if (request.token == address(0)) {
            // Rút ETH
            (bool success, ) = request.recipient.call{value: request.amount}("");
            if (!success) revert TokenTransferFailed();
        } else {
            // Rút token
            IERC20(request.token).safeTransfer(request.recipient, request.amount);
        }
        
        emit EmergencyWithdrawal(request.token, request.recipient, request.amount);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }
    
    /**
     * @dev Get all supported tokens
     * @return Array of supported token addresses
     */
    function getSupportedTokens() external view returns (address[] memory) {
        return supportedTokensList;
    }
    
    /**
     * @dev Pause bridge
     */
    function pause() external onlyOwner {
        _pause();
    }
    
    /**
     * @dev Unpause bridge
     */
    function unpause() external onlyOwner {
        _unpause();
    }
    
    /**
     * @dev Nhận ETH
     */
    receive() external payable {}

    /**
     * @dev Add a supported token for bridging
     * @param _token Địa chỉ token
     */
    function addSupportedToken(address _token) external onlyOwner {
        if (_token == address(0)) revert ZeroAddress();
<<<<<<< HEAD
        if (supportedTokens[_token]) return;
=======
        if (supportedTokens[_token]) return; // Token đã được hỗ trợ
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        
        supportedTokens[_token] = true;
        supportedTokensList.push(_token);
        
<<<<<<< HEAD
        emit TokenTypeSupported(_token, TOKEN_TYPE_ERC20, msg.sender, block.timestamp);
=======
        emit TokenTypeSupported(_token, TOKEN_TYPE_ERC20);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }

    /**
     * @dev Remove a supported token
     * @param _token Địa chỉ token
     */
    function removeSupportedToken(address _token) external onlyOwner {
<<<<<<< HEAD
        if (!supportedTokens[_token]) return;
        
        supportedTokens[_token] = false;
        
        for (uint256 i = 0; i < supportedTokensList.length; i++) {
            if (supportedTokensList[i] == _token) {
=======
        if (!supportedTokens[_token]) return; // Token chưa được hỗ trợ
        
        supportedTokens[_token] = false;
        
        // Cập nhật lại danh sách
        for (uint256 i = 0; i < supportedTokensList.length; i++) {
            if (supportedTokensList[i] == _token) {
                // Xóa token khỏi danh sách bằng cách thay thế bằng token cuối cùng
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
                supportedTokensList[i] = supportedTokensList[supportedTokensList.length - 1];
                supportedTokensList.pop();
                break;
            }
        }
        
<<<<<<< HEAD
        emit TokenTypeSupported(_token, 0, msg.sender, block.timestamp);
=======
        emit TokenTypeSupported(_token, 0); // 0 = không hỗ trợ
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }

    /**
     * @dev Set the default fee for a specific chain
     * @param _chainId The LayerZero chain ID
     * @param _baseFee The base fee in wei
     * @param _feePercentage The fee percentage in basis points
     */
    function setDefaultFee(uint16 _chainId, uint256 _baseFee, uint256 _feePercentage) external onlyOwner {
        if (_feePercentage > MAX_FEE_BPS) revert InvalidFeeBps();
        defaultFees[_chainId] = Fee(_baseFee, _feePercentage);
<<<<<<< HEAD
        emit DefaultFeesUpdated(_chainId, defaultFees[_chainId].baseFee, _baseFee, defaultFees[_chainId].feePercentage, _feePercentage, msg.sender, block.timestamp);
=======
        emit DefaultFeesUpdated(_chainId, _baseFee, _feePercentage);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }

    /**
     * @dev Estimate the fee for bridging tokens to a specific chain
     * @param _dstChainId The destination chain ID
     * @param _amount The amount of tokens to bridge
     * @return The estimated fee
     */
    function estimateFee(uint16 _dstChainId, uint256 _amount) public view returns (uint256) {
        Fee memory fee = defaultFees[_dstChainId];
        return fee.baseFee + (_amount * fee.feePercentage / FEE_DENOMINATOR);
    }

    /**
     * @dev Set bridge coordinator address and enable/disable usage
     * @param _coordinator The bridge coordinator address
     * @param _useCoordinator Whether to use the coordinator
     */
    function setBridgeCoordinator(address _coordinator, bool _useCoordinator) external onlyOwner {
        if (_useCoordinator && _coordinator == address(0)) revert ZeroAddress();
        
        address oldCoordinator = address(bridgeCoordinator);
        bridgeCoordinator = IBridgeCoordinator(_coordinator);
        useCoordinator = _useCoordinator;
        
<<<<<<< HEAD
        emit CoordinatorUpdated(oldCoordinator, _coordinator, msg.sender, block.timestamp);
        emit CoordinatorStatusChanged(_useCoordinator, _useCoordinator, msg.sender, block.timestamp);
=======
        emit CoordinatorUpdated(oldCoordinator, _coordinator);
        emit CoordinatorStatusChanged(_useCoordinator);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }
    
    /**
     * @dev Tạo bridge ID duy nhất cho mỗi giao dịch bridge
     * @param _sender Người gửi
     * @param _dstChainId Chain ID đích
     * @param _toAddress Địa chỉ đích
     * @param _amount Số lượng token
     * @return Mã ID duy nhất cho giao dịch bridge
     */
    function _generateBridgeId(
        address _sender,
        uint16 _dstChainId,
        bytes memory _toAddress,
        uint256 _amount
    ) internal view returns (bytes32) {
<<<<<<< HEAD
        // Thêm nonce để tránh trùng lặp
        uint64 nonce = nextNonce[uint16(block.chainid)][_sender];
        
=======
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        return keccak256(abi.encode(
            _sender,
            _dstChainId,
            _toAddress,
            _amount,
            block.timestamp,
<<<<<<< HEAD
            block.number,
            nonce
=======
            block.number
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        ));
    }
    
    /**
     * @dev Lưu trữ thông tin bridge operation
     * @param _bridgeId ID của bridge operation
     * @param _sender Người gửi
     * @param _token Địa chỉ token
     * @param _dstChainId Chain ID đích
     * @param _toAddress Địa chỉ đích
     * @param _amount Số lượng token
     * @param _status Trạng thái operation
     */
    function _recordBridgeOperation(
        bytes32 _bridgeId,
        address _sender,
        address _token,
        uint16 _dstChainId,
        bytes memory _toAddress,
        uint256 _amount,
        string memory _status
    ) internal {
<<<<<<< HEAD
=======
        // Lưu thông tin vào local storage cho bridge interface
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        BridgeOperation storage operation = bridgeOperations[_bridgeId];
        operation.sender = _sender;
        operation.token = _token;
        operation.destinationChainId = _dstChainId;
        operation.destinationAddress = _toAddress;
        operation.amount = _amount;
        operation.timestamp = block.timestamp;
        operation.completed = false;
        operation.status = _status;
        
<<<<<<< HEAD
        userBridgeOperations[_sender].push(_bridgeId);
        
        if (allBridgeOperations.length >= MAX_OPERATIONS) {
            _cleanupCompletedOperations();
        }
        
        allBridgeOperations.push(_bridgeId);
        
=======
        // Thêm vào danh sách theo địa chỉ
        userBridgeOperations[_sender].push(_bridgeId);
        
        // Thêm vào danh sách tất cả
        allBridgeOperations.push(_bridgeId);
        
        // Nếu có coordinator, ghi lại thông tin
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        if (useCoordinator && address(bridgeCoordinator) != address(0)) {
            bridgeCoordinator.recordBridgeOperation(
                _bridgeId,
                _sender,
                _token,
                _dstChainId,
                _toAddress,
                _amount,
                _status
            );
        }
        
<<<<<<< HEAD
=======
        // Bug #13 Fix: Emit event để đồng bộ giữa các contracts
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        emit BridgeOperationCreated(
            _bridgeId,
            _sender,
            _token,
            _dstChainId,
            _toAddress,
            _amount,
            _status,
            block.timestamp
        );
    }
    
    /**
<<<<<<< HEAD
     * @dev Dọn dẹp các bridge operation đã hoàn thành
     * @return Số lượng bridge operation đã dọn dẹp
     */
    function _cleanupCompletedOperations() internal returns (uint256) {
        uint256 cleanupCount = 0;
        uint256 limit = 50;
        
        for (uint256 i = allBridgeOperations.length; i > 0 && cleanupCount < limit; i--) {
            bytes32 opId = allBridgeOperations[i - 1];
            BridgeOperation memory op = bridgeOperations[opId];
            
            if (op.completed || op.timestamp < block.timestamp - MAX_STORAGE_TIME) {
                delete bridgeOperations[opId];
                
                if (op.sender != address(0)) {
                    bytes32[] storage userOps = userBridgeOperations[op.sender];
                    for (uint256 j = 0; j < userOps.length; j++) {
                        if (userOps[j] == opId) {
                            if (j < userOps.length - 1) {
                                userOps[j] = userOps[userOps.length - 1];
                            }
                            userOps.pop();
                            break;
                        }
                    }
                }
                
                if (i != allBridgeOperations.length) {
                    allBridgeOperations[i - 1] = allBridgeOperations[allBridgeOperations.length - 1];
                }
                allBridgeOperations.pop();
                
                cleanupCount++;
            }
        }
        
        return cleanupCount;
    }
    
    /**
=======
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
     * @dev Cập nhật trạng thái bridge operation
     * @param _bridgeId ID của bridge operation
     * @param _status Trạng thái mới
     */
    function _updateBridgeStatus(bytes32 _bridgeId, string memory _status) internal {
        BridgeOperation storage operation = bridgeOperations[_bridgeId];
<<<<<<< HEAD
        if (operation.timestamp == 0) return;
=======
        if (operation.timestamp == 0) return; // Operation not found
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        
        operation.status = _status;
        operation.completed = keccak256(abi.encodePacked(_status)) == keccak256(abi.encodePacked("COMPLETED"));
        
<<<<<<< HEAD
=======
        // Nếu có coordinator, cập nhật trạng thái
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        if (useCoordinator && address(bridgeCoordinator) != address(0)) {
            bridgeCoordinator.updateBridgeStatus(_bridgeId, _status);
        }
        
<<<<<<< HEAD
=======
        // Bug #13 Fix: Emit event để đồng bộ giữa các contracts
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        emit BridgeOperationUpdated(
            _bridgeId,
            _status,
            operation.completed,
            block.timestamp
        );
        
<<<<<<< HEAD
        emit BridgeStatusUpdated(_bridgeId, _status, block.timestamp);
    }

    // Thay thế hàm encode/decode payload cũ bằng chuẩn hóa dùng BridgePayloadCodec
    using BridgePayloadCodec for *;
    using BridgePayloadCodec for BridgePayload;

    // Hàm tạo payload chuẩn hóa
    function _encodeBridgePayload(BridgePayload memory payload) internal pure returns (bytes memory) {
        return BridgePayloadCodec.encode(payload);
    }
    // Hàm giải mã payload chuẩn hóa
    function _decodeBridgePayload(bytes memory encoded) internal pure returns (BridgePayload memory) {
        return BridgePayloadCodec.decode(encoded);
=======
        // Emit trạng thái bridge
        emit BridgeStatusUpdated(_bridgeId, _status, block.timestamp);
    }

    /**
     * @dev Mã hóa payload chuẩn cho bridge
     * @param _sender Người gửi
     * @param _toAddress Địa chỉ đích
     * @param _amount Số lượng token
     * @param _token Địa chỉ token
     * @return Payload đã mã hóa
     */
    function _encodeBridgePayload(
        address _sender,
        bytes memory _toAddress,
        uint256 _amount,
        address _token
    ) internal view returns (bytes memory) {
        return abi.encode(
            _sender,                    // sender
            _toAddress,                 // destination
            _amount,                    // amount
            block.timestamp,            // timestamp
            address(0),                 // unwrapper (mặc định là không unwrap)
            false,                      // needUnwrap flag
            tokenTypes[_token]          // token type
        );
    }

    /**
     * @dev Giải mã payload chuẩn từ bridge
     * @param _payload Payload cần giải mã
     * @return sender, amount, timestamp, token
     */
    function _decodeBridgePayload(bytes memory _payload) internal pure returns (
        address sender,
        uint256 amount,
        uint256 timestamp,
        address token
    ) {
        bytes memory toAddress;
        bool needUnwrap;
        address unwrapper;
        uint8 tokenType;

        // Decode với định dạng mới
        try abi.decode(_payload, (address, bytes, uint256, uint256, address, bool, uint8)) returns (
            address _sender,
            bytes memory _toAddress,
            uint256 _amount,
            uint256 _timestamp,
            address _unwrapper,
            bool _needUnwrap,
            uint8 _tokenType
        ) {
            return (_sender, _amount, _timestamp, address(0));
        } catch {
            // Nếu không phải định dạng mới, thử với định dạng cũ
            try abi.decode(_payload, (address, bytes, uint256, uint256)) returns (
                address _sender,
                bytes memory _toAddress,
                uint256 _amount,
                uint256 _timestamp
            ) {
                return (_sender, _amount, _timestamp, address(0));
            } catch {
                // Không thể decode, trả về giá trị mặc định
                return (address(0), 0, 0, address(0));
            }
        }
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }

    /**
     * @dev Kiểm tra các tham số bridge có hợp lệ không
     * @param _dstChainId Chain ID đích
     * @param _toAddress Địa chỉ đích
     * @param _amount Số lượng token
     */
    function _checkBridgeParams(uint16 _dstChainId, bytes calldata _toAddress, uint256 _amount) internal view {
        if (_toAddress.length == 0) revert EmptyDestination();
        if (_amount == 0) revert InvalidAmount();
        if (_amount > transactionLimit) revert ExceededRateLimit();
<<<<<<< HEAD
        if (!ChainRegistry.isChainSupported(chainRegistry, _dstChainId)) revert TrustedRemoteNotSet();
        
        bool withinLimit = _checkDailyVolume(_amount);
=======
        if (trustedRemotes[_dstChainId].length == 0) revert TrustedRemoteNotSet();
        
        // Kiểm tra rate limit
        bool withinLimit = _checkAndUpdateDailyVolume(_amount);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        if (!withinLimit) revert ExceededRateLimit();
    }

    /**
     * @dev Kiểm tra khối lượng hàng ngày không vượt quá giới hạn
     * @param _amount Số lượng token
     * @return Có trong giới hạn không
     */
    function _checkDailyVolume(uint256 _amount) internal view returns (bool) {
        uint256 currentDay = _getCurrentDay();
        uint256 newVolume = dailyVolume[currentDay] + _amount;
        return newVolume <= dailyLimit;
    }

    /**
     * @dev Kiểm tra trusted remote có hợp lệ không
     * @param _srcChainId Chain ID nguồn
     * @param _srcAddress Địa chỉ nguồn
     */
    function _checkTrustedRemote(uint16 _srcChainId, bytes memory _srcAddress) internal view {
<<<<<<< HEAD
        bytes memory trustedRemote = ChainRegistry.getRemoteAddress(chainRegistry, _srcChainId);
=======
        bytes memory trustedRemote = trustedRemotes[_srcChainId];
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        if (trustedRemote.length == 0) revert TrustedRemoteNotSet();
        if (keccak256(_srcAddress) != keccak256(trustedRemote)) revert InvalidSourceAddress();
    }

    /**
     * @dev Kiểm tra timestamp có hợp lệ không
     * @param _timestamp Timestamp cần kiểm tra
     */
    function _checkTimestamp(uint256 _timestamp) internal view {
        if (block.timestamp > _timestamp + EXPIRY_WINDOW) revert InvalidTimestamp();
    }

    /**
     * @dev Chuyển đổi bytes thành address
     * @param _toBytes Bytes cần chuyển đổi
     * @return Địa chỉ đã chuyển đổi
     */
    function _toAddress(bytes memory _toBytes) internal pure returns (address) {
        if (_toBytes.length < 20) revert InvalidDestinationAddress();
        address to;
        assembly {
            to := mload(add(_toBytes, 20))
        }
        return to;
    }

    /**
     * @dev Lưu thông tin giao dịch thất bại
     * @param _srcChainId Chain ID nguồn
     * @param _to Địa chỉ người nhận
     * @param _amount Số lượng token
     * @param _nonce Nonce của giao dịch
     * @param _reason Lý do thất bại
     */
    function _storeFailedTransaction(
<<<<<<< HEAD
        uint16 _srcChainId,
        address _to,
        uint256 _amount,
        uint64 _nonce,
        string memory _reason
    ) internal returns (bytes32) {
        bytes32 txId = keccak256(abi.encodePacked(
            _srcChainId,
            _to,
            _amount,
            _nonce,
            _reason,
=======
        address token,
        TokenType tokenType,
        address sender,
        uint16 srcChainId,
        uint16 dstChainId,
        bytes memory toAddress,
        uint256 amount,
        uint256 fee,
        bytes memory payload
    ) internal returns (bytes32) {
        // Create a unique transaction ID using the current timestamp for uniqueness
        bytes32 txId = keccak256(abi.encodePacked(
            token,
            tokenType,
            sender,
            srcChainId,
            dstChainId,
            toAddress,
            amount,
            fee,
            payload,
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
            block.timestamp
        ));
        
        failedTransactions[txId] = FailedTransaction({
<<<<<<< HEAD
            srcChainId: _srcChainId,
            to: _to,
            amount: _amount,
            nonce: _nonce,
            reason: _reason,
            timestamp: block.timestamp,
            processed: false
        });
        
        if (failedTransactionIds.length >= MAX_FAILED_TXS) {
            bytes32 oldestTxId = failedTransactionIds[0];
            
            for (uint256 i = 0; i < failedTransactionIds.length - 1; i++) {
                failedTransactionIds[i] = failedTransactionIds[i + 1];
            }
            
            failedTransactionIds[failedTransactionIds.length - 1] = txId;
            
            emit FailedTransactionLimitReached(oldestTxId, txId, failedTransactionIds.length, block.timestamp);
        } else {
=======
            token: token,
            tokenType: tokenType,
            sender: sender,
            srcChainId: srcChainId,
            dstChainId: dstChainId,
            toAddress: toAddress,
            amount: amount,
            fee: fee,
            payload: payload,
            isProcessed: false,
            timestamp: block.timestamp
        });
        
        // Handle array size limit - if we've reached the limit, replace the oldest transaction
        if (failedTransactionIds.length >= MAX_FAILED_TXS) {
            bytes32 oldestTxId = failedTransactionIds[0];
            // Shift all elements to the left, effectively removing the first element
            for (uint256 i = 0; i < failedTransactionIds.length - 1; i++) {
                failedTransactionIds[i] = failedTransactionIds[i + 1];
            }
            // Replace the last element with the new txId
            failedTransactionIds[failedTransactionIds.length - 1] = txId;
            // Emit event to notify that we've reached the limit
            emit FailedTransactionLimitReached(oldestTxId, txId);
        } else {
            // If we haven't reached the limit, simply push the new txId
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
            failedTransactionIds.push(txId);
        }
        
        emit FailedTransactionStored(
<<<<<<< HEAD
            _srcChainId,
            _to,
            _amount,
            _nonce,
            _reason,
            block.timestamp
=======
            txId,
            token,
            tokenType,
            sender,
            srcChainId,
            dstChainId,
            toAddress,
            amount,
            fee
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        );
        
        return txId;
    }

    /**
     * @dev Thực hiện bridge token đến chain đích
     * @param _token Địa chỉ token
     * @param _tokenId Token ID (cho ERC721 và ERC1155)
     * @param _amount Số lượng token 
     * @param _dstChainId Chain ID đích
     * @param _toAddress Địa chỉ đích
     * @param _needUnwrap Có cần unwrap tại chain đích không
     * @param _adapterParams Params cho adapter
     */
    function bridgeToken(
        address _token,
        uint256 _tokenId,
        uint256 _amount,
        uint16 _dstChainId,
        bytes memory _toAddress,
        bool _needUnwrap,
        bytes memory _adapterParams
    ) external payable whenNotPaused nonReentrant {
        if (_token == address(0)) revert ZeroAddress();
        if (_toAddress.length == 0) revert EmptyDestination();
<<<<<<< HEAD
        // Validate địa chỉ cross-chain
        if (_toAddressFromBytes(_toAddress, _dstChainId) == address(0)) revert InvalidDestinationAddress();
        if (_amount == 0) revert InvalidAmount();
        if (!ChainRegistry.isChainSupported(chainRegistry, _dstChainId)) revert TrustedRemoteNotSet();
=======
        if (_amount == 0) revert InvalidAmount();
        if (!trustedRemotes[_dstChainId].length > 0) revert TrustedRemoteNotSet();
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        if (blacklistedTokens[_token]) revert UnsupportedToken();
        if (!whitelistedTokens[_token]) revert UnsupportedToken();
        if (_amount > transactionLimit) revert ExceededRateLimit();
        
        uint8 tokenType = tokenTypes[_token];
        if (tokenType == 0) revert UnsupportedToken();
        
<<<<<<< HEAD
        if (!_checkAndUpdateDailyVolume(_amount)) revert ExceededRateLimit();
        
=======
        // Kiểm tra giới hạn giao dịch
        if (!_checkAndUpdateDailyVolume(_amount)) revert ExceededRateLimit();
        
        // Tính phí
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        uint256 bridgeFee = calculateFee(_amount);
        uint256 amountAfterFee = _amount - bridgeFee;
        if (amountAfterFee == 0) revert FeeExceedsAmount();
        
<<<<<<< HEAD
=======
        // Tạo bridge ID
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        bytes32 bridgeId = keccak256(abi.encode(
            msg.sender,
            _token,
            _dstChainId,
            _toAddress,
            _amount,
            _tokenId,
            block.timestamp
        ));
        
<<<<<<< HEAD
=======
        // Lưu thông tin bridge operation
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        _recordBridgeOperation(
            bridgeId,
            msg.sender,
            _token,
            _dstChainId,
            _toAddress,
            _amount,
            "Initiated"
        );
        
        // Xử lý các loại token khác nhau
        if (tokenType == TOKEN_TYPE_ERC20) {
            // Chuyển token ERC20 từ người dùng vào bridge
            IERC20(_token).safeTransferFrom(msg.sender, address(this), _amount);
            
            // Chuyển phí cho feeCollector
            if (bridgeFee > 0 && feeCollector != address(0)) {
                IERC20(_token).safeTransfer(feeCollector, bridgeFee);
            }
        } else if (tokenType == TOKEN_TYPE_ERC1155) {
            // Chuyển token ERC1155 từ người dùng vào bridge
            IERC1155(_token).safeTransferFrom(msg.sender, address(this), _tokenId, _amount, "");
            
            // Không tính phí cho ERC1155 vì phức tạp
        } else if (tokenType == TOKEN_TYPE_NATIVE) {
            // Yêu cầu đủ ETH
            if (msg.value < _amount) revert InvalidAmount();
            
            // Chuyển phí cho feeCollector
            if (bridgeFee > 0 && feeCollector != address(0)) {
                (bool success, ) = feeCollector.call{value: bridgeFee}("");
                if (!success) revert TokenTransferFailed();
            }
        } else if (tokenType == TOKEN_TYPE_ERC721) {
            // Kiểm tra chủ sở hữu
            if (IERC721(_token).ownerOf(_tokenId) != msg.sender) revert NotAuthorized();
            
            // Kiểm tra approval
            if (!IERC721(_token).isApprovedForAll(msg.sender, address(this)) && 
                IERC721(_token).getApproved(_tokenId) != address(this)) revert InsufficientAllowance();
            
            // Chuyển token ERC721
            IERC721(_token).safeTransferFrom(msg.sender, address(this), _tokenId, "");
            
            // ERC721 không tính phí vì là NFT
        } else if (tokenType == TOKEN_TYPE_ERC777) {
            // Kiểm tra operator
            if (!IERC777(_token).isOperatorFor(address(this), msg.sender)) revert InsufficientAllowance();
            
            // Chuyển token ERC777
            IERC777(_token).operatorSend(msg.sender, address(this), _amount, "", "");
            
            // Chuyển phí
            if (bridgeFee > 0 && feeCollector != address(0)) {
                IERC777(_token).operatorSend(address(this), feeCollector, bridgeFee, "", "");
            }
        }
        
        // Chuẩn bị payload
        bytes memory payload = abi.encode(
            msg.sender,
            _toAddress,
            _amount,
            block.timestamp,
            _token,
            tokenType,
            _tokenId,
            _needUnwrap
        );
        
        // Gửi qua LayerZero
<<<<<<< HEAD
        bytes memory trustedRemote = ChainRegistry.getRemoteAddress(chainRegistry, _dstChainId);
=======
        bytes memory trustedRemote = trustedRemotes[_dstChainId];
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        
        uint256 estimatedFee = _estimateGasFee(_dstChainId, msg.sender, payload, _adapterParams, 0);
        if (msg.value < estimatedFee) revert InsufficientGasFee();
        
        _lzSend(
            _dstChainId,
            payload,
            payable(msg.sender),
            address(0x0),
            _adapterParams,
            msg.value
        );
        
        // Emit event
        emit BridgeInitiated(
            bridgeId,
            msg.sender,
            _token,
            _amount,
            _dstChainId,
            _toAddress,
            bridgeFee,
            block.timestamp
        );
    }

    /**
     * @dev Bridge native token từ chain này sang chain khác
     * @param _dstChainId Chain ID đích
     * @param _toAddress Địa chỉ đích
     * @param _adapterParams Params cho adapter
     */
    function bridgeNative(
        uint16 _dstChainId,
        bytes memory _toAddress,
        bytes memory _adapterParams
    ) external payable whenNotPaused nonReentrant {
        if (_toAddress.length == 0) revert EmptyDestination();
<<<<<<< HEAD
        // Validate địa chỉ cross-chain
        if (_toAddressFromBytes(_toAddress, _dstChainId) == address(0)) revert InvalidDestinationAddress();
        if (msg.value == 0) revert InvalidAmount();
        if (!ChainRegistry.isChainSupported(chainRegistry, _dstChainId)) revert TrustedRemoteNotSet();
=======
        if (msg.value == 0) revert InvalidAmount();
        if (!trustedRemotes[_dstChainId].length > 0) revert TrustedRemoteNotSet();
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        
        uint256 amount = msg.value;
        
        // Kiểm tra giới hạn
        if (amount > transactionLimit) revert ExceededRateLimit();
        if (!_checkAndUpdateDailyVolume(amount)) revert ExceededRateLimit();
        
        // Tính phí
        uint256 bridgeFee = calculateFee(amount);
        uint256 amountAfterFee = amount - bridgeFee;
        if (amountAfterFee == 0) revert FeeExceedsAmount();
        
        // Tạo bridge ID
        bytes32 bridgeId = keccak256(abi.encode(
            msg.sender,
            address(0),
            _dstChainId,
            _toAddress,
            amount,
            0,
            block.timestamp
        ));
        
        // Lưu thông tin bridge operation
        _recordBridgeOperation(
            bridgeId,
            msg.sender,
            address(0),
            _dstChainId,
            _toAddress,
            amount,
            "Initiated"
        );
        
        // Chuyển phí cho feeCollector
        if (bridgeFee > 0 && feeCollector != address(0)) {
            (bool success, ) = feeCollector.call{value: bridgeFee}("");
            if (!success) revert TokenTransferFailed();
        }
        
        // Chuẩn bị payload
        bytes memory payload = abi.encode(
            msg.sender,
            _toAddress,
            amountAfterFee,
            block.timestamp,
            address(0),
            TOKEN_TYPE_NATIVE,
            0,
            false
        );
        
        // Ước tính phí gas
        uint256 estimatedGasFee = _estimateGasFee(_dstChainId, msg.sender, payload, _adapterParams, 0);
        
        // Kiểm tra đủ gas
        if (msg.value < (amountAfterFee + estimatedGasFee)) revert InsufficientGasFee();
        
        // Gửi qua LayerZero (trừ đi số token cần chuyển)
        _lzSend(
            _dstChainId,
            payload,
            payable(msg.sender),
            address(0x0),
            _adapterParams,
            estimatedGasFee
        );
        
        // Emit event
        emit BridgeInitiated(
            bridgeId,
            msg.sender,
            address(0),
            amount,
            _dstChainId,
            _toAddress,
            bridgeFee,
            block.timestamp
        );
    }

    /**
     * @dev Cập nhật token được hỗ trợ cho mỗi chain
     * @param _chainId Chain ID
     * @param _token Token address
     */
    function updateWrappedTokenForChain(uint16 _chainId, address _token) external onlyOwner {
        if (_token == address(0)) revert ZeroAddress();
<<<<<<< HEAD
        // Thay thế các chỗ kiểm tra trustedRemotes[_dstChainId] bằng ChainRegistry.isChainSupported
        // Ví dụ:
        // if (trustedRemotes[_dstChainId].length == 0) revert TrustedRemoteNotSet();
        // =>
        // if (!ChainRegistry.isChainSupported(chainRegistry, _dstChainId)) revert TrustedRemoteNotSet();
        // Thay thế wrappedTokenByChain và unwrapperRegistry tương tự bằng các hàm get/set của ChainRegistry nếu cần (có thể mở rộng struct ChainInfo nếu cần lưu thêm address unwrapper/token)
        // ...
        emit TokenAddressUpdated(wrappedTokenByChain[_chainId], _token, _chainId, msg.sender, block.timestamp);
=======
        wrappedTokenByChain[_chainId] = _token;
        emit TokenAddressUpdated(wrappedTokenByChain[_chainId], _token);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }

    /**
     * @dev Lấy token được hỗ trợ cho một chain
     * @param _chainId Chain ID
     * @return Địa chỉ token
     */
    function getWrappedTokenForChain(uint16 _chainId) external view returns (address) {
<<<<<<< HEAD
        address token = ChainRegistry.getRemoteAddress(chainRegistry, _chainId);
=======
        address token = wrappedTokenByChain[_chainId];
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        if (token == address(0)) {
            return wrappedDMDToken;
        }
        return token;
    }

    /**
     * @dev Process auto-unwrap request after receiving bridged tokens
     * @param _to Recipient address
     * @param _amount Token amount
     * @param _unwrapper Unwrapper contract address
     */
<<<<<<< HEAD
    function processAutoUnwrap(address _to, uint256 _amount, uint16 chainId) internal nonReentrant {
        address _unwrapper = ChainRegistry.getUnwrapper(chainRegistry, chainId);
        if (_unwrapper == address(0)) revert UnwrapperNotRegistered(chainId);
        if (_amount == 0) return;
        // --- Rate limit check ---
        uint256 currentDay = block.timestamp / 1 days;
        if (dailyUnwrapVolume[currentDay] + _amount > dailyUnwrapLimit) revert ExceededRateLimit();
        dailyUnwrapVolume[currentDay] += _amount;
        // --- Generate unique unwrapId with nonce for uniqueness ---
        uint64 nonce = nextNonce[uint16(block.chainid)][_to];
        bytes32 unwrapId = keccak256(abi.encode(_to, _amount, _unwrapper, block.timestamp, nonce));
        nextNonce[uint16(block.chainid)][_to]++;
        // Update status: UNWRAPPING
        _updateBridgeStatus(unwrapId, BridgeStatus.UNWRAPPING, "");
        // --- Use SafeERC20 for approve and check allowance ---
        IERC20 token = IERC20(wrappedDMDToken);
        token.safeApprove(_unwrapper, 0); // reset to 0 for safety
        token.safeApprove(_unwrapper, _amount);
        if (token.allowance(address(this), _unwrapper) < _amount) revert InsufficientAllowance();
            try IUnwrapper(_unwrapper).unwrap(_to, _amount) {
            emit TokenUnwrapped(unwrapId, _to, wrappedDMDToken, _amount, block.timestamp);
            emit AutoUnwrapProcessed(unwrapId, _to, _unwrapper, _amount, true, block.timestamp);
                _updateBridgeStatus(unwrapId, BridgeStatus.COMPLETED, "");
            } catch Error(string memory reason) {
                _updateBridgeStatus(unwrapId, BridgeStatus.FAILED, reason);
            emit AutoUnwrapFailed(wrappedDMDToken, _to, _amount, reason);
            emit AutoUnwrapProcessed(unwrapId, _to, _unwrapper, _amount, false, block.timestamp);
                revert UnwrapFailed(wrappedDMDToken, _unwrapper, _amount);
            } catch {
                _updateBridgeStatus(unwrapId, BridgeStatus.FAILED, "Unknown error");
            emit AutoUnwrapFailed(wrappedDMDToken, _to, _amount, "Unknown error");
            emit AutoUnwrapProcessed(unwrapId, _to, _unwrapper, _amount, false, block.timestamp);
            revert UnwrapFailed(wrappedDMDToken, _unwrapper, _amount);
        }
    }

    /**
     * @dev Generate transaction ID with enhanced security
     * Includes nonce to prevent replay attacks following SolGuard requirements
     */
    function generateTxId(
        address sender,
        uint16 srcChainId,
        uint16 dstChainId,
        address recipient,
            uint256 amount,
        uint256 timestamp
    ) public view returns (bytes32) {
        // Get next nonce for this sender-chain pair
        uint64 nonce = nextNonce[dstChainId][sender];
        
        // Generate unique transaction ID that includes nonce
        return keccak256(abi.encode(
            sender,
            srcChainId,
            dstChainId,
            recipient,
            amount,
            timestamp,
            nonce
        ));
    }

    /**
     * @dev Record transaction state update
     * Following SolGuard's CEI pattern
     */
    function recordTransactionState(
        bytes32 txId,
        uint8 status,
        uint16 sourceChain,
        uint16 destinationChain,
        address sender,
        address recipient,
        uint256 amount,
        string memory errorMsg
    ) public onlyOwner nonReentrant {
        // Create state record
        TransactionState storage state = transactionStates[txId];
        
        // Update state if it doesn't exist or status is progressing forward
        if (state.timestamp == 0 || state.status < status) {
            bytes memory signature = _signState(txId, status, block.timestamp);
            bytes32 stateRoot = generateStateRoot(txId, status, block.timestamp);
            
            state.txId = txId;
            state.status = status;
            state.timestamp = block.timestamp;
            state.signature = signature;
            state.validator = msg.sender;
            state.stateRoot = stateRoot;
            state.sourceChain = sourceChain;
            state.destinationChain = destinationChain;
            state.sender = sender;
            state.recipient = recipient;
            state.amount = amount;
            
            if (status == 3) { // Failed
                state.errorMessage = errorMsg;
            }
            
            // Emit event
            emit TransactionStateChanged(txId, status, block.timestamp, msg.sender);
        }
    }

    /**
     * @dev Create confirmation payload to send back to source chain
     */
    function createConfirmationPayload(
        bytes32 txId, 
        uint8 status,
        uint16 srcChainId,
        uint16 dstChainId
    ) public view returns (bytes memory) {
        // Create signature validating this state
        bytes memory signature = _signState(txId, status, block.timestamp);
        
        // Generate state root for verification
        bytes32 stateRoot = generateStateRoot(txId, status, block.timestamp);
        
        // Prepare confirmation payload - starts with MESSAGE_TYPE_CONFIRMATION
        return abi.encodePacked(
            uint8(MESSAGE_TYPE_CONFIRMATION), // Message type byte
            abi.encode(
                txId,
                status,
                block.timestamp,
                signature,
                address(this),
                stateRoot,
                srcChainId,
                dstChainId
            )
        );
    }

    /**
     * @dev Send confirmation back to source chain
     */
    function sendConfirmation(
        bytes32 txId, 
        uint8 status,
        uint16 srcChainId,
        uint16 dstChainId,
        address sender
    ) internal {
        bytes memory payload = createConfirmationPayload(txId, status, srcChainId, dstChainId);
        
        // Send confirmation message
        _lzSend(
            srcChainId, 
            payload, 
            payable(sender), 
            address(0), 
            bytes(""), 
            0.01 ether
        );
        
        // Emit event
        emit ConfirmationSent(txId, srcChainId, dstChainId, status, block.timestamp);
    }

    /**
     * @dev Create emergency payload
     */
    function createEmergencyPayload(
        bytes32 txId,
        string memory reason,
        bytes memory recoveryData
    ) public view onlyOwner returns (bytes memory) {
        // Create emergency message payload
        return abi.encodePacked(
            uint8(MESSAGE_TYPE_EMERGENCY), // Message type byte
            abi.encode(
                txId,
                reason,
                recoveryData,
                block.timestamp,
                address(this)
            )
        );
    }

    /**
     * @dev Process confirmation message
     */
    function _processConfirmation(
        uint16 srcChainId,
        bytes memory srcAddress,
        bytes memory payload
    ) internal nonReentrant {
        // Decode confirmation payload
        (
            bytes32 txId,
            uint8 status,
            uint256 timestamp,
            bytes memory signature,
            address validator,
            bytes32 stateRoot,
            uint16 sourceCh,
            uint16 destCh
        ) = abi.decode(payload, (bytes32, uint8, uint256, bytes, address, bytes32, uint16, uint16));
        
        // Verify trusted remote
        if (!_isTrustedRemote(srcChainId, srcAddress)) revert InvalidSourceAddress();
        
        // Check for expired confirmation
        if (block.timestamp - timestamp > CONFIRMATION_EXPIRY) revert ConfirmationExpired();
        
        // Create confirmationId to prevent replay
        bytes32 confirmationId = keccak256(abi.encode(txId, srcChainId, status, timestamp));
        if (processedConfirmations[confirmationId]) revert ConfirmationAlreadyProcessed();
        
        // Verify signature
        if (!verifySignature(txId, status, timestamp, signature, validator)) revert InvalidSignature();
        
        // Verify state root
        if (!verifyStateRoot(stateRoot, txId, status, timestamp)) revert InvalidStateRoot();
        
        // Mark confirmation as processed
        processedConfirmations[confirmationId] = true;
        
        // Update transaction status only if status is advancing
        TransactionState storage state = transactionStates[txId];
        if (state.timestamp == 0 || state.status < status) {
            state.status = status;
            state.timestamp = block.timestamp;
            state.signature = signature;
            state.validator = validator;
            state.stateRoot = stateRoot;
            
            if (status == 3) { // Failed
                // Handle failed transaction
                _handleFailedTransaction(txId);
            }
        }
        
        // Emit event
        emit ConfirmationReceived(txId, srcChainId, status, block.timestamp);
    }

    /**
     * @dev Handle failed transaction
     */
    function _handleFailedTransaction(bytes32 txId) internal {
        // Logic to handle failed transaction
        // Could include retry logic, refund logic, etc.
        // For now, just update the bridge status
        _updateBridgeStatus(txId, BridgeStatus.FAILED, "Transaction failed on destination chain");
    }

    /**
     * @dev Slice bytes - Helper function to extract part of a byte array
     */
    function _slice(
        bytes memory _bytes,
        uint256 _start,
        uint256 _length
    ) internal pure returns (bytes memory) {
        require(_length + _start <= _bytes.length, "Slice out of bounds");
        
        bytes memory tempBytes = new bytes(_length);
        
        for (uint256 i = 0; i < _length; i++) {
            tempBytes[i] = _bytes[_start + i];
        }
        
        return tempBytes;
    }

    /**
     * @dev Thêm failed transaction vào danh sách quản lý
     * @param txId ID của giao dịch thất bại
     */
    function _addFailedTransaction(bytes32 txId) internal {
        // Kiểm tra xem txId đã tồn tại trong danh sách chưa
        for (uint256 i = 0; i < failedTransactionIds.length; i++) {
            if (failedTransactionIds[i] == txId) {
                return; // Đã tồn tại, không thêm nữa
            }
        }
        
        // Nếu đạt đến giới hạn, áp dụng chiến lược FIFO hoặc dọn dẹp
        if (failedTransactionIds.length >= MAX_FAILED_TXS) {
            // Trước tiên, cố gắng xóa các giao dịch đã xử lý
            bool cleaned = _cleanupProcessedFailedTransactions();
            
            // Nếu không thể dọn dẹp (tất cả vẫn chưa được xử lý), loại bỏ giao dịch cũ nhất
            if (!cleaned) {
                bytes32 oldestTxId = failedTransactionIds[0];
                
                // Dịch chuyển các phần tử còn lại
                for (uint256 i = 0; i < failedTransactionIds.length - 1; i++) {
                    failedTransactionIds[i] = failedTransactionIds[i + 1];
                }
                
                // Thêm txId mới vào cuối
                failedTransactionIds[failedTransactionIds.length - 1] = txId;
                
                emit FailedTransactionLimitReached(oldestTxId, txId, failedTransactionIds.length, block.timestamp);
                return;
            }
        }
        
        // Thêm txId mới vào danh sách
        failedTransactionIds.push(txId);
    }

    /**
     * @dev Dọn dẹp các giao dịch đã xử lý trong danh sách
     * @return Có dọn dẹp được ít nhất một giao dịch không
     */
    function _cleanupProcessedFailedTransactions() internal returns (bool) {
        bool anyProcessed = false;
        uint256 processedCount = 0;
        uint256 i = 0;
        
        while (i < failedTransactionIds.length) {
            if (failedTransactions[failedTransactionIds[i]].processed) {
                // Xóa phần tử hiện tại bằng cách dịch chuyển các phần tử còn lại
                for (uint256 j = i; j < failedTransactionIds.length - 1; j++) {
                    failedTransactionIds[j] = failedTransactionIds[j + 1];
                }
                
                // Giảm kích thước mảng
                failedTransactionIds.pop();
                
                anyProcessed = true;
                processedCount++;
                // Không tăng i vì phần tử tại vị trí i hiện tại đã thay đổi
            } else {
                i++;
            }
        }
        
        if (anyProcessed) {
            emit ProcessedFailedTransactionsCleared(processedCount, msg.sender, block.timestamp);
        }
        
        return anyProcessed;
=======
    function processAutoUnwrap(address _to, uint256 _amount, address _unwrapper) internal {
        if (_unwrapper == address(0) || _amount == 0) return;
        
        // Tạo bridge ID cho operation này
        bytes32 unwrapId = keccak256(abi.encode(_to, _unwrapper, _amount, block.timestamp));
        
        bool success = false;
        try IWrappedDMD(wrappedDMDToken).approve(_unwrapper, _amount) {
            try IErc1155Wrapper(_unwrapper).unwrap(_amount) {
                success = true;
            } catch {
                success = false;
            }
        } catch {
            success = false;
        }
        
        // Bug #13 Fix: Emit event để đồng bộ giữa các contracts
        emit AutoUnwrapProcessed(
            unwrapId,
            _to,
            _unwrapper,
            _amount,
            success,
            block.timestamp
        );
    }

    /**
     * @dev Xử lý nhận tin nhắn từ LayerZero
     * @param _srcChainId Chain ID nguồn
     * @param _srcAddress Địa chỉ nguồn (trusted remote)
     * @param _nonce Nonce của tin nhắn
     * @param _payload Payload của tin nhắn
     */
    function _nonblockingLzReceive(
        uint16 _srcChainId,
        bytes memory _srcAddress,
        uint64 _nonce,
        bytes memory _payload
    ) internal override {
        // Kiểm tra source address có hợp lệ
        if (trustedRemotes[_srcChainId].length == 0) revert TrustedRemoteNotSet();
        if (keccak256(_srcAddress) != keccak256(trustedRemotes[_srcChainId])) revert InvalidSourceAddress();
        
        // Kiểm tra nonce đã xử lý chưa
        if (processedNonces[_srcChainId][_nonce]) revert NonceAlreadyProcessed();
        processedNonces[_srcChainId][_nonce] = true;
        
        // Giải mã payload
        (
            address fromAddress,
            bytes memory toAddressBytes,
            uint256 amount,
            uint256 timestamp,
            address tokenAddress,
            uint8 tokenType,
            uint256 tokenId,
            bool needUnwrap
        ) = abi.decode(_payload, (address, bytes, uint256, uint256, address, uint8, uint256, bool));
        
        // Kiểm tra thời gian hết hạn
        if (block.timestamp > timestamp + EXPIRY_WINDOW) revert InvalidTimestamp();
        
        // Kiểm tra giới hạn giao dịch
        if (amount > transactionLimit) revert ExceededRateLimit();
        if (!_checkAndUpdateDailyVolume(amount)) revert ExceededRateLimit();
        
        // Chuyển đổi địa chỉ đích từ bytes sang address
        address toAddress;
        assembly {
            toAddress := mload(add(toAddressBytes, 20))
        }
        
        if (toAddress == address(0)) revert ZeroAddress();
        
        // Tạo bridge ID
        bytes32 bridgeId = keccak256(abi.encode(
            fromAddress,
            tokenAddress,
            _srcChainId,
            toAddress,
            amount,
            tokenId,
            timestamp
        ));
        
        // Thực hiện bridge
        try {
            // Kiểm tra loại token
            if (tokenType == TOKEN_TYPE_ERC20) {
                // Nếu là token mặc định (WrappedDMD)
                if (tokenAddress == address(0) || tokenAddress == wrappedDMDToken) {
                    // Mint wrapped token cho người nhận
                    IWrappedDMD wrappedDMD = IWrappedDMD(wrappedDMDToken);
                    
                    // Kiểm tra quyền mint
                    if (wrappedDMD.verifyProxyOperation(1)) {
                        wrappedDMD.mint(toAddress, amount);
                        
                        // Nếu needUnwrap = true, unwrap token về ERC1155
                        if (needUnwrap) {
                            _processAutoUnwrap(tokenAddress, toAddress, amount, _srcChainId);
                        }
                    } else {
                        revert MintFailed("Proxy operation not verified");
                    }
                } else if (whitelistedTokens[tokenAddress]) {
                    // Mint hoặc transfer token được whitelist
                    if (tokenTypes[tokenAddress] == TOKEN_TYPE_ERC20) {
                        IERC20(tokenAddress).safeTransfer(toAddress, amount);
                    } else {
                        revert UnsupportedToken();
                    }
                } else {
                    revert UnsupportedToken();
                }
            } else if (tokenType == TOKEN_TYPE_ERC1155) {
                // Transfer ERC1155 token cho người nhận
                if (whitelistedTokens[tokenAddress] && tokenTypes[tokenAddress] == TOKEN_TYPE_ERC1155) {
                    IERC1155(tokenAddress).safeTransferFrom(address(this), toAddress, tokenId, amount, "");
                } else {
                    revert UnsupportedToken();
                }
            } else if (tokenType == TOKEN_TYPE_NATIVE) {
                // Transfer native token (ETH/BNB/etc) cho người nhận
                (bool success, ) = toAddress.call{value: amount}("");
                if (!success) revert TokenTransferFailed();
            } else if (tokenType == TOKEN_TYPE_ERC721) {
                // Transfer ERC721 token cho người nhận
                if (whitelistedTokens[tokenAddress] && tokenTypes[tokenAddress] == TOKEN_TYPE_ERC721) {
                    IERC721(tokenAddress).safeTransferFrom(address(this), toAddress, tokenId, "");
                } else {
                    revert UnsupportedToken();
                }
            } else if (tokenType == TOKEN_TYPE_ERC777) {
                // Transfer ERC777 token cho người nhận
                if (whitelistedTokens[tokenAddress] && tokenTypes[tokenAddress] == TOKEN_TYPE_ERC777) {
                    IERC777(tokenAddress).operatorSend(address(this), toAddress, amount, "", "");
                } else {
                    revert UnsupportedToken();
                }
            } else {
                revert UnsupportedToken();
            }
            
            // Lưu thông tin bridge operation
            _recordBridgeOperation(
                bridgeId,
                fromAddress,
                tokenAddress == address(0) ? wrappedDMDToken : tokenAddress,
                _srcChainId,
                abi.encodePacked(toAddress),
                amount,
                "Completed"
            );
            
            // Emit event
            emit BridgeReceived(
                bridgeId,
                fromAddress,
                toAddress,
                amount,
                _srcChainId,
                _nonce,
                block.timestamp
            );
            
            emit BridgeStatusUpdated(
                bridgeId,
                "Success",
                block.timestamp
            );
        } catch (bytes memory reason) {
            // Xử lý lỗi
            string memory errorMessage = _getRevertMsg(reason);
            
            // Lưu thông tin token bị mắc kẹt
            bytes32 failedTxId = keccak256(abi.encode(_srcChainId, toAddress, amount, _nonce));
            failedTransactions[failedTxId] = FailedTransaction({
                srcChainId: _srcChainId,
                to: toAddress,
                amount: amount,
                nonce: _nonce,
                reason: errorMessage,
                timestamp: block.timestamp,
                processed: false
            });
            
            emit FailedMint(_srcChainId, toAddress, amount, errorMessage);
            
            // Lưu thông tin bridge operation
            _recordBridgeOperation(
                bridgeId,
                fromAddress,
                tokenAddress == address(0) ? wrappedDMDToken : tokenAddress,
                _srcChainId,
                abi.encodePacked(toAddress),
                amount,
                "Failed"
            );
            
            emit FailedTransactionStored(_srcChainId, toAddress, amount, _nonce, errorMessage, block.timestamp);
            
            emit BridgeStatusUpdated(
                bridgeId,
                "Failed",
                block.timestamp
            );
            
            // Revert giao dịch
            revert MintFailed(errorMessage);
        }
    }

    /**
     * @dev Xử lý unwrap tự động
     * @param token Địa chỉ token
     * @param recipient Địa chỉ nhận token
     * @param amount Số lượng token cần unwrap
     * @param chainId Chain ID hiện tại
     * @return Kết quả xử lý unwrap (thành công hay thất bại)
     */
    function _processAutoUnwrap(
        address token, 
        address recipient, 
        uint256 amount, 
        uint16 chainId
    ) internal returns (bool) {
        // Get the registered unwrapper for this chain
        address unwrapper = getUnwrapper(chainId);
        
        // Check if an unwrapper is registered for this chain
        if (unwrapper == address(0)) {
            revert UnwrapperNotRegistered(chainId);
        }
        
        // Verify that the unwrapper is a contract
        if (!Address.isContract(unwrapper)) {
            revert UnwrapperNotContract(unwrapper);
        }
        
        // Approve tokens for the unwrapper
        try IERC20(token).approve(unwrapper, amount) returns (bool success) {
            if (!success) {
                emit AutoUnwrapFailed(token, recipient, amount, "Token approval failed");
                return false;
            }
            
            // Call the unwrap function
            try IUnwrapper(unwrapper).unwrap(token, recipient, amount) returns (bool unwrapSuccess) {
                if (unwrapSuccess) {
                    emit AutoUnwrapProcessed(token, recipient, amount, unwrapper);
                    return true;
                } else {
                    emit AutoUnwrapFailed(token, recipient, amount, "Unwrap function returned false");
                    return false;
                }
            } catch Error(string memory reason) {
                emit AutoUnwrapFailed(token, recipient, amount, reason);
                
                // Revoke approval if unwrap failed
                IERC20(token).approve(unwrapper, 0);
                return false;
            } catch (bytes memory) {
                emit AutoUnwrapFailed(token, recipient, amount, "Unknown error in unwrap call");
                
                // Revoke approval if unwrap failed
                IERC20(token).approve(unwrapper, 0);
                return false;
            }
        } catch Error(string memory reason) {
            emit AutoUnwrapFailed(token, recipient, amount, string(abi.encodePacked("Approval failed: ", reason)));
            return false;
        } catch (bytes memory) {
            emit AutoUnwrapFailed(token, recipient, amount, "Unknown error in approval");
            return false;
        }
    }
    
    /**
     * @dev Ước tính phí gas
     * @param _dstChainId Chain ID đích
     * @param _userApplication Địa chỉ người dùng
     * @param _payload Payload
     * @param _adapterParams Adapter params
     * @param _payInZRO Pay in ZRO
     * @return Phí gas
     */
    function _estimateGasFee(
        uint16 _dstChainId,
        address _userApplication,
        bytes memory _payload,
        bytes memory _adapterParams,
        uint _payInZRO
    ) internal view returns (uint) {
        (uint nativeFee, ) = lzEndpoint.estimateFees(
            _dstChainId,
            address(this),
            _payload,
            _payInZRO,
            _adapterParams
        );
        return nativeFee;
    }
    
    /**
     * @dev Giải mã thông báo lỗi
     * @param _returnData Dữ liệu trả về khi revert
     * @return Thông báo lỗi
     */
    function _getRevertMsg(bytes memory _returnData) internal pure returns (string memory) {
        // Nếu _returnData ít hơn 68 bytes, không phải là revert với thông báo lỗi
        if (_returnData.length < 68) return "Transaction reverted silently";
        
        assembly {
            // Nhảy qua 4 bytes của selector function
            _returnData := add(_returnData, 0x04)
        }
        
        // Giải mã và trả về thông báo lỗi
        return abi.decode(_returnData, (string));
    }

    /**
     * @dev Lấy danh sách ID của các giao dịch thất bại với phân trang
     * @param offset Vị trí bắt đầu
     * @param limit Số lượng phần tử tối đa cần lấy
     * @return Mảng ID các giao dịch thất bại
     */
    function getFailedTransactionIds(uint256 offset, uint256 limit) external view returns (bytes32[] memory) {
        require(offset < failedTransactionIds.length, "Offset out of bounds");
        
        // Calculate the actual limit based on array size
        uint256 actualLimit = limit;
        if (offset + limit > failedTransactionIds.length) {
            actualLimit = failedTransactionIds.length - offset;
        }
        
        bytes32[] memory result = new bytes32[](actualLimit);
        for (uint256 i = 0; i < actualLimit; i++) {
            result[i] = failedTransactionIds[offset + i];
        }
        
        return result;
    }
    
    /**
     * @dev Lấy thông tin chi tiết về một giao dịch thất bại
     * @param txId ID của giao dịch thất bại
     * @return Thông tin về giao dịch thất bại
     */
    function getFailedTransaction(bytes32 txId) external view returns (FailedTransaction memory) {
        require(failedTransactions[txId].timestamp > 0, "Transaction not found");
        return failedTransactions[txId];
    }
    
    /**
     * @dev Đánh dấu một giao dịch thất bại đã được xử lý
     * @param txId ID của giao dịch thất bại
     */
    function markFailedTransactionProcessed(bytes32 txId) external onlyOwner {
        require(failedTransactions[txId].timestamp > 0, "Transaction not found");
        require(!failedTransactions[txId].processed, "Transaction already processed");
        
        failedTransactions[txId].processed = true;
        emit FailedTransactionProcessed(txId);
    }
    
    /**
     * @dev Xóa các giao dịch thất bại đã được xử lý khỏi danh sách
     */
    function clearProcessedFailedTransactions() external onlyOwner {
        uint256 processedCount = 0;
        uint256 newIndex = 0;
        
        // Create a temporary array to avoid gas issues with deleting array elements
        bytes32[] memory newIds = new bytes32[](failedTransactionIds.length);
        
        // Filter out processed transactions
        for (uint256 i = 0; i < failedTransactionIds.length; i++) {
            bytes32 txId = failedTransactionIds[i];
            if (!failedTransactions[txId].processed) {
                newIds[newIndex] = txId;
                newIndex++;
            } else {
                processedCount++;
            }
        }
        
        // Rebuild the array with only unprocessed transactions
        if (processedCount > 0) {
            // Clear the existing array
            while (failedTransactionIds.length > 0) {
                failedTransactionIds.pop();
            }
            
            // Add back only the unprocessed transactions
            for (uint256 i = 0; i < newIndex; i++) {
                failedTransactionIds.push(newIds[i]);
            }
            
            emit ProcessedFailedTransactionsCleared(processedCount);
        }
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }

    /**
     * @notice Register an unwrapper for a specific chain
     * @param chainId The chain ID to register for
     * @param unwrapper The address of the unwrapper contract
     */
    function registerUnwrapper(uint16 chainId, address unwrapper) external onlyOwner {
        require(unwrapper != address(0), "Zero address not allowed");
        require(Address.isContract(unwrapper), "Unwrapper must be a contract");
<<<<<<< HEAD
        // Đảm bảo không trùng lặp
        address oldUnwrapper = ChainRegistry.getUnwrapper(chainRegistry, chainId);
        require(oldUnwrapper == address(0) || oldUnwrapper != unwrapper, "Unwrapper already registered");
        ChainRegistry.setUnwrapper(chainRegistry, chainId, unwrapper);
        emit UnwrapperRegistered(chainId, unwrapper, msg.sender, block.timestamp);
=======
        unwrapperRegistry[chainId] = unwrapper;
        emit UnwrapperRegistered(chainId, unwrapper);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }

    /**
     * @notice Get the registered unwrapper for a specific chain
     * @param chainId The chain ID to get the unwrapper for
     * @return The unwrapper address or zero address if not registered
     */
    function getUnwrapper(uint16 chainId) public view returns (address) {
<<<<<<< HEAD
        return ChainRegistry.getRemoteAddress(chainRegistry, chainId);
=======
        return unwrapperRegistry[chainId];
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }

    /**
     * @notice Converts bytes to address safely
     * @param addressBytes The bytes to convert to address
     * @param chainId The chain ID to determine the format
     * @return The converted address
     */
    function _toAddressFromBytes(bytes memory addressBytes, uint16 chainId) internal pure returns (address) {
        // Check if the address bytes are empty
        if (addressBytes.length == 0) revert InvalidDestinationAddress();
        
        // Check if it's a NEAR chain or other non-EVM chain format
        if (_isNearChain(chainId)) {
            // For NEAR chains, we need special handling
            // NEAR addresses might be longer than 20 bytes, so we check for minimum length
            if (addressBytes.length < 20) revert InvalidDestinationAddress();
            
            // We take the first 20 bytes as the Ethereum address
            address result;
            assembly {
                result := mload(add(addressBytes, 20))
            }
            return result;
        } else {
            // For EVM chains, strict validation
            if (addressBytes.length != 20) revert InvalidDestinationAddress();
            
            address result;
            assembly {
                result := mload(add(addressBytes, 20))
            }
            return result;
        }
    }

    /**
     * @notice Determines if a chain ID is for a NEAR chain
     * @param chainId The chain ID to check
     * @return True if it's a NEAR chain
     */
    function _isNearChain(uint16 chainId) internal pure returns (bool) {
        // NEAR Chains: Aurora = 211, NEAR Protocol = 235
        return chainId == 211 || chainId == 235;
    }

    /**
     * @notice Remove an unwrapper from the registry
     * @param chainId The chain ID to remove the unwrapper for
     */
    function removeUnwrapper(uint16 chainId) external onlyOwner {
<<<<<<< HEAD
        address oldUnwrapper = ChainRegistry.getRemoteAddress(chainRegistry, chainId);
        delete ChainRegistry.setChain(chainRegistry, chainId, ChainRegistry.getChainName(chainRegistry, chainId), "", false);
        emit UnwrapperRemoved(chainId, oldUnwrapper, msg.sender, block.timestamp);
    }

    /**
     * @dev Dọn dẹp các bridge operation cũ
     * @param _threshold Ngưỡng thời gian (tính bằng giây) để xác định operation cũ
     */
    function cleanupOldOperations(uint256 _threshold) external onlyOwner {
        require(_threshold >= 1 days, "Threshold too low");
        require(_threshold <= MAX_STORAGE_TIME, "Threshold too high");
        
        uint256 cleanupCount = 0;
        uint256 timestampThreshold = block.timestamp - _threshold;
        
        // Clean up from the end of the array to optimize gas
        for (uint256 i = allBridgeOperations.length; i > 0; i--) {
            bytes32 opId = allBridgeOperations[i - 1];
            BridgeOperation memory op = bridgeOperations[opId];
            
            if (op.timestamp < timestampThreshold || op.completed) {
                // Delete from mapping
                delete bridgeOperations[opId];
                
                // Remove from userBridgeOperations
                bytes32[] storage userOps = userBridgeOperations[op.sender];
                for (uint256 j = 0; j < userOps.length; j++) {
                    if (userOps[j] == opId) {
                        // Shift remaining elements
                        for (uint256 k = j; k < userOps.length - 1; k++) {
                            userOps[k] = userOps[k + 1];
                        }
                        userOps.pop();
                        break;
                    }
                }
                
                // Remove from allBridgeOperations
                // If i - 1 is not the last element
                if (i != allBridgeOperations.length) {
                    allBridgeOperations[i - 1] = allBridgeOperations[allBridgeOperations.length - 1];
                }
                allBridgeOperations.pop();
                
                cleanupCount++;
                
                // Limit the number of cleanups in one transaction to avoid out of gas
                if (cleanupCount >= 100) {
                    break;
                }
            }
        }
        
        emit BridgeOperationsCleaned(cleanupCount, _threshold, msg.sender, block.timestamp);
    }

    // Need to add getTokenAddress() function that will be overridden in child contracts
    function getTokenAddress() public virtual returns (address) {
        revert("BridgeInterface: getTokenAddress must be overridden in derived contracts");
    }

    /**
     * @dev Mint tokens for receiver (only bridge or owner, checks permissions, status, balance, dynamic gas, event)
     * @param to Recipient address
     * @param amount Token amount
     * @param bridgeId Bridge operation ID
     */
    function mintTokens(address to, uint256 amount, bytes32 bridgeId) external nonReentrant {
        // Only allow bridge or owner to call
        if (msg.sender != bridgeCoordinator && msg.sender != owner()) revert NotAuthorized();
        if (to == address(0)) revert ZeroAddress();
        if (amount == 0) revert InvalidAmount();
        // Check mint permission (according to .solguard standard)
        if (!IWrappedDMD(wrappedDMDToken).verifyProxyOperation(1)) revert NotAuthorized();
        // Update status before minting
        _updateBridgeStatus(bridgeId, "MINTING");
        // Check balance before minting
        uint256 balanceBefore = IERC20(wrappedDMDToken).balanceOf(to);
        // Mint token
        try IWrappedDMD(wrappedDMDToken).mint(to, amount) {
            uint256 balanceAfter = IERC20(wrappedDMDToken).balanceOf(to);
            if (balanceAfter < balanceBefore + amount) revert MintFailed("Minted amount mismatch");
            _updateBridgeStatus(bridgeId, "COMPLETED");
            emit BridgeStatusUpdated(bridgeId, "COMPLETED", block.timestamp);
        } catch Error(string memory reason) {
            _updateBridgeStatus(bridgeId, string(abi.encodePacked("FAILED: ", reason)));
            emit BridgeStatusUpdated(bridgeId, string(abi.encodePacked("FAILED: ", reason)), block.timestamp);
            revert MintFailed(reason);
        } catch {
            _updateBridgeStatus(bridgeId, "FAILED: Unknown error");
            emit BridgeStatusUpdated(bridgeId, "FAILED: Unknown error", block.timestamp);
            revert MintFailed("Unknown error");
        }
    }

    /**
     * @dev Clean up completed bridge operations with batch size and gas check
     * @param _batchSize Maximum number of operations to clean up in one transaction
     * @return Number of bridge operations cleaned up
     */
    function cleanupBridgeOperations(uint256 _batchSize) external onlyOwner returns (uint256) {
        uint256 cleanupCount = 0;
        uint256 limit = _batchSize > 0 ? _batchSize : 50;
        uint256 GAS_THRESHOLD = 100000; // Gas threshold to stop cleaning
        
        for (uint256 i = allBridgeOperations.length; i > 0 && cleanupCount < limit; i--) {
            // Check remaining gas
            if (gasleft() < GAS_THRESHOLD) {
                break;
            }
            
            bytes32 opId = allBridgeOperations[i - 1];
            BridgeOperation memory op = bridgeOperations[opId];
            
            // Prioritize old data
            if (op.completed || op.timestamp < block.timestamp - MAX_STORAGE_TIME) {
                delete bridgeOperations[opId];
                
                if (op.sender != address(0)) {
                    bytes32[] storage userOps = userBridgeOperations[op.sender];
                    for (uint256 j = 0; j < userOps.length; j++) {
                        if (userOps[j] == opId) {
                            if (j < userOps.length - 1) {
                                userOps[j] = userOps[userOps.length - 1];
                            }
                            userOps.pop();
                            break;
                        }
                    }
                }
                
                if (i != allBridgeOperations.length) {
                    allBridgeOperations[i - 1] = allBridgeOperations[allBridgeOperations.length - 1];
                }
                allBridgeOperations.pop();
                
                cleanupCount++;
                
                // Emit event for tracking
                emit BridgeOperationsCleaned(1, allBridgeOperations.length, msg.sender, block.timestamp);
            }
        }
        
        return cleanupCount;
    }

    /**
     * @dev Read message type and check payload version
     * Simplified to leverage LayerZero's validation
     */
    function _getMessageType(bytes memory _payload) internal pure returns (uint8, uint8) {
        if (_payload.length < 2) {
            return (0, 0);
        }
        
        // First byte is the version
        uint8 version;
        // Second byte is the message type
        uint8 messageType;
        
        assembly {
            version := mload(add(_payload, 1))
            messageType := mload(add(_payload, 2))
        }
        
        return (version, messageType);
    }

    /**
     * @dev Process the message based on the message type
     * Simplified validation, trusting LayerZero
     */
    function _processLzMessage(
        uint16 _srcChainId,
        bytes memory _srcAddress,
        uint64 _nonce,
        bytes memory _payload
    ) internal override nonReentrant {
        // Anti-replay: check and save nonce
        if (processedNonces[_srcChainId][_nonce]) revert NonceAlreadyProcessed();
        processedNonces[_srcChainId][_nonce] = true;
        // Decode message type from payload
        (uint8 version, uint8 messageType) = _getMessageType(_payload);
        
        // Check payload version
        if (version == 0) {
            // Payload has no version byte, handle in legacy way for backward compatibility
            // Only read the first byte as message type
            assembly {
                messageType := mload(add(_payload, 1))
            }
        } else if (version != PAYLOAD_VERSION) {
            // Unsupported version
            _storeFailedTransaction(
                _srcChainId,
                address(0),
                0,
                _nonce,
                string(abi.encodePacked("Unsupported payload version: ", version))
            );
            return;
        }
        
        // Check message type and payload length
        if (messageType == 0 || messageType > 3) {
            _storeFailedTransaction(
                _srcChainId,
                address(0),
                0,
                _nonce,
                string(abi.encodePacked("Invalid message type: ", messageType))
            );
            return;
        }
        
        // Check minimum length based on message type
        if (_payload.length < EXPECTED_LENGTH_BY_TYPE[messageType]) {
            _storeFailedTransaction(
                _srcChainId,
                address(0),
                0,
                _nonce,
                string(abi.encodePacked("Payload too short for message type: ", messageType))
            );
            return;
        }
        
        // Process by message type
        bytes memory actualPayload = _payload;
        
        // Skip version and message type bytes if version exists
        if (version > 0) {
            actualPayload = new bytes(_payload.length - 2);
            for (uint i = 0; i < actualPayload.length; i++) {
                actualPayload[i] = _payload[i + 2];
            }
        } else {
            // Skip only message type byte for old version payload
            actualPayload = new bytes(_payload.length - 1);
            for (uint i = 0; i < actualPayload.length; i++) {
                actualPayload[i] = _payload[i + 1];
            }
        }
        
        // Process by message type
        if (messageType == MESSAGE_TYPE_BRIDGE) {
            // Bridge message
            _processBridgeMessage(_srcChainId, _srcAddress, _nonce, actualPayload);
        } else if (messageType == MESSAGE_TYPE_CONFIRMATION) {
            // Confirmation message
            _processConfirmation(_srcChainId, _srcAddress, actualPayload);
        } else if (messageType == MESSAGE_TYPE_EMERGENCY) {
            // Emergency message
            _processEmergencyMessage(_srcChainId, _srcAddress, _nonce, actualPayload);
        }
    }

    /**
     * @dev Create payload with version byte
     * @param messageType Type of message (1=Bridge, 2=Confirmation, 3=Emergency)
     * @param payload Actual payload data
     * @return bytes Formatted payload with version and type
     */
    function _createPayloadWithVersion(uint8 messageType, bytes memory payload) internal view returns (bytes memory) {
        return abi.encodePacked(
            PAYLOAD_VERSION,  // Version byte
            messageType,      // Message type byte
            payload           // Actual payload
        );
    }

    /**
     * @dev Get current day (Unix timestamp divided by seconds in 1 day)
     * @return Current day
     */
    function _getCurrentDay() internal view returns (uint256) {
        return block.timestamp / 1 days;
    }
    
    /**
     * @dev Check and update daily volume
     * @param amount Token amount
     * @return Whether within limit
     */
    function _checkAndUpdateDailyVolume(uint256 amount) internal returns (bool) {
        uint256 currentDay = _getCurrentDay();
        uint256 newVolume = dailyVolume[currentDay] + amount;
        
        if (newVolume > dailyLimit) {
            return false;
        }
        
        dailyVolume[currentDay] = newVolume;
        return true;
    }

    /**
     * @dev Check and update daily volume
     * @param amount Token amount
     * @return Whether within limit
     */
    function _checkDailyVolume(uint256 _amount) internal view returns (bool) {
        uint256 currentDay = _getCurrentDay();
        uint256 newVolume = dailyVolume[currentDay] + _amount;
        return newVolume <= dailyLimit;
    }

    /**
     * @dev Check if trusted remote is valid
     * @param _srcChainId Source chain ID
     * @param _srcAddress Source address
     */
    function _checkTrustedRemote(uint16 _srcChainId, bytes memory _srcAddress) internal view {
        bytes memory trustedRemote = ChainRegistry.getRemoteAddress(chainRegistry, _srcChainId);
        if (trustedRemote.length == 0) revert TrustedRemoteNotSet();
        if (keccak256(_srcAddress) != keccak256(trustedRemote)) revert InvalidSourceAddress();
    }

    /**
     * @dev Check if timestamp is valid
     * @param _timestamp Timestamp to check
     */
    function _checkTimestamp(uint256 _timestamp) internal view {
        if (block.timestamp > _timestamp + EXPIRY_WINDOW) revert InvalidTimestamp();
    }

    /**
     * @dev Convert bytes to address
     * @param _toBytes Bytes to convert
     * @return Converted address
     */
    function _toAddress(bytes memory _toBytes) internal pure returns (address) {
        if (_toBytes.length < 20) revert InvalidDestinationAddress();
        address to;
        assembly {
            to := mload(add(_toBytes, 20))
        }
        return to;
    }

    /**
     * @dev Store failed transaction information
     * @param _srcChainId Source chain ID
     * @param _to Recipient address
     * @param _amount Token amount
     * @param _nonce Transaction nonce
     * @param _reason Failure reason
     */
    function _storeFailedTransaction(
        uint16 _srcChainId,
        address _to,
        uint256 _amount,
        uint64 _nonce,
        string memory _reason
    ) internal returns (bytes32) {
        bytes32 txId = keccak256(abi.encodePacked(
            _srcChainId,
            _to,
            _amount,
            _nonce,
            _reason,
            block.timestamp
        ));
        
        failedTransactions[txId] = FailedTransaction({
            srcChainId: _srcChainId,
            to: _to,
            amount: _amount,
            nonce: _nonce,
            reason: _reason,
            timestamp: block.timestamp,
            processed: false
        });
        
        if (failedTransactionIds.length >= MAX_FAILED_TXS) {
            bytes32 oldestTxId = failedTransactionIds[0];
            
            for (uint256 i = 0; i < failedTransactionIds.length - 1; i++) {
                failedTransactionIds[i] = failedTransactionIds[i + 1];
            }
            
            failedTransactionIds[failedTransactionIds.length - 1] = txId;
            
            emit FailedTransactionLimitReached(oldestTxId, txId, failedTransactionIds.length, block.timestamp);
        } else {
            failedTransactionIds.push(txId);
        }
        
        emit FailedTransactionStored(
            _srcChainId,
            _to,
            _amount,
            _nonce,
            _reason,
            block.timestamp
        );
        
        return txId;
    }

    /**
     * @dev Clean up completed bridge operations
     * @return Number of bridge operations cleaned up
     */
    function _cleanupCompletedOperations() internal returns (uint256) {
        uint256 cleanupCount = 0;
        uint256 limit = 50;
        
        for (uint256 i = allBridgeOperations.length; i > 0 && cleanupCount < limit; i--) {
            bytes32 opId = allBridgeOperations[i - 1];
            BridgeOperation memory op = bridgeOperations[opId];
            
            if (op.completed || op.timestamp < block.timestamp - MAX_STORAGE_TIME) {
                delete bridgeOperations[opId];
                
                if (op.sender != address(0)) {
                    bytes32[] storage userOps = userBridgeOperations[op.sender];
                    for (uint256 j = 0; j < userOps.length; j++) {
                        if (userOps[j] == opId) {
                            if (j < userOps.length - 1) {
                                userOps[j] = userOps[userOps.length - 1];
                            }
                            userOps.pop();
                            break;
                        }
                    }
                }
                
                if (i != allBridgeOperations.length) {
                    allBridgeOperations[i - 1] = allBridgeOperations[allBridgeOperations.length - 1];
                }
                allBridgeOperations.pop();
                
                cleanupCount++;
            }
        }
        
        return cleanupCount;
    }
    
    /**
     * @dev Update bridge operation status
     * @param _bridgeId Bridge operation ID
     * @param _status New status
     */
    function _updateBridgeStatus(bytes32 _bridgeId, string memory _status) internal {
        BridgeOperation storage operation = bridgeOperations[_bridgeId];
        if (operation.timestamp == 0) return;
        
        operation.status = _status;
        operation.completed = keccak256(abi.encodePacked(_status)) == keccak256(abi.encodePacked("COMPLETED"));
        
        if (useCoordinator && address(bridgeCoordinator) != address(0)) {
            bridgeCoordinator.updateBridgeStatus(_bridgeId, _status);
        }
        
        emit BridgeOperationUpdated(
            _bridgeId,
            _status,
            operation.completed,
            block.timestamp
        );
        
        emit BridgeStatusUpdated(_bridgeId, _status, block.timestamp);
    }

    /**
     * @dev Check bridge parameters validity
     * @param _dstChainId Destination chain ID
     * @param _toAddress Destination address
     * @param _amount Token amount
     */
    function _checkBridgeParams(uint16 _dstChainId, bytes calldata _toAddress, uint256 _amount) internal view {
        if (_toAddress.length == 0) revert EmptyDestination();
        if (_amount == 0) revert InvalidAmount();
        if (_amount > transactionLimit) revert ExceededRateLimit();
        if (!ChainRegistry.isChainSupported(chainRegistry, _dstChainId)) revert TrustedRemoteNotSet();
        
        bool withinLimit = _checkDailyVolume(_amount);
        if (!withinLimit) revert ExceededRateLimit();
    }

    // Variables for bridge and LayerZero
    address public wrappedDMDToken;
    uint256 public feePercentage = 0; 
    uint256 public constant MAX_FEE = 100; 
    address public feeCollector;
    // Replace chainId-related mappings with standardized registry
    ChainRegistry.Registry private chainRegistry;
    // mapping(uint16 => bytes) public trustedRemotes; // REMOVED
    // mapping(uint16 => address) public wrappedTokenByChain; // REMOVED
    // mapping(uint16 => address) public unwrapperRegistry; // REMOVED
    
    // Configure expected payload lengths by message type
    mapping(uint8 => uint256) public EXPECTED_LENGTH_BY_TYPE;

    // Add version byte at the beginning of each payload
    uint8 public constant PAYLOAD_VERSION = 1;
    
    // Mapping to store processed nonces for each chainId (anti-replay)
    mapping(uint16 => mapping(uint64 => bool)) public processedNonces;
    
    // Ensure standardized rescue, stuck, emergency
    // Already exists: requestEmergencyWithdrawal, executeEmergencyWithdrawal, emergencyWithdrawals, event EmergencyWithdrawal, EMERGENCY_TIMELOCK, onlyOwner, limit, timeout
    // Add events for rescuing stuck ERC20/ERC1155/ETH tokens if not exists

    /**
     * @dev Update wrapped token for a specific chain
     * @param _chainId Chain ID
     * @param _token Token address
     */
    function updateWrappedTokenForChain(uint16 _chainId, address _token) external onlyOwner {
        if (_token == address(0)) revert ZeroAddress();
        // Replace trusted remote checks with ChainRegistry.isChainSupported
        // Example:
        // if (trustedRemotes[_dstChainId].length == 0) revert TrustedRemoteNotSet();
        // =>
        // if (!ChainRegistry.isChainSupported(chainRegistry, _dstChainId)) revert TrustedRemoteNotSet();
        // Similarly replace wrappedTokenByChain and unwrapperRegistry with ChainRegistry get/set functions if needed
        emit TokenAddressUpdated(wrappedTokenByChain[_chainId], _token, _chainId, msg.sender, block.timestamp);
    }

    /**
     * @dev Get supported token for a chain
     * @param _chainId Chain ID
     * @return Token address
     */
    function getWrappedTokenForChain(uint16 _chainId) external view returns (address) {
        address token = ChainRegistry.getRemoteAddress(chainRegistry, _chainId);
        if (token == address(0)) {
            return wrappedDMDToken;
        }
        return token;
    }

    /**
     * @dev Request trusted remote update (with timelock)
     * @param _chainId Chain ID
     * @param _remoteAddress Remote address
     */
    function requestTrustedRemoteUpdate(uint16 _chainId, bytes calldata _remoteAddress) external onlyOwner {
        if (_remoteAddress.length == 0) revert EmptyDestination();
        // Use ChainRegistry to update remoteAddress
        ChainRegistry.setChain(chainRegistry, _chainId, ChainRegistry.getChainName(chainRegistry, _chainId), _remoteAddress, true);
        emit TrustedRemoteUpdateRequested(_chainId, _remoteAddress, block.timestamp + TIMELOCK_DURATION, msg.sender, block.timestamp);
    }
    
    /**
     * @dev Execute trusted remote update request after timelock period expires
     * @param _chainId Chain ID
     */
    function executeTrustedRemoteUpdate(uint16 _chainId) external onlyOwner {
        // Get remoteAddress from registry
        bytes memory remoteAddress = ChainRegistry.getRemoteAddress(chainRegistry, _chainId);
        if (remoteAddress.length == 0) revert EmptyDestination();
        // Ensure only owner can update
        ChainRegistry.setChain(chainRegistry, _chainId, ChainRegistry.getChainName(chainRegistry, _chainId), remoteAddress, true);
        emit TrustedRemoteUpdated(_chainId, remoteAddress, msg.sender, block.timestamp);
    }
    
    /**
     * @dev Cancel trusted remote update request
     * @param _chainId Chain ID
     */
    function cancelTrustedRemoteUpdate(uint16 _chainId) external onlyOwner {
        // Set supported = false
        ChainRegistry.setChain(chainRegistry, _chainId, ChainRegistry.getChainName(chainRegistry, _chainId), "", false);
        emit TrustedRemoteUpdateCanceled(_chainId, msg.sender, block.timestamp);
=======
        address oldUnwrapper = unwrapperRegistry[chainId];
        delete unwrapperRegistry[chainId];
        emit UnwrapperRemoved(chainId, oldUnwrapper);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }
}

// Define the IUnwrapper interface for better type safety
interface IUnwrapper {
    function unwrap(address token, address recipient, uint256 amount) external returns (bool);
} 