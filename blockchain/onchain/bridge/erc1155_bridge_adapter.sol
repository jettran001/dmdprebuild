// SPDX-License-Identifier: MIT
pragma solidity 0.8.20;

import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/security/Pausable.sol";
import "@openzeppelin/contracts/security/ReentrancyGuard.sol";
import "@layerzerolabs/solidity-examples/contracts/lzApp/NonblockingLzApp.sol";
import "./bridge_adapter/IBridgeAdapter.sol";
import "@openzeppelin/contracts/token/ERC1155/IERC1155.sol";
import "@openzeppelin/contracts/token/ERC1155/utils/ERC1155Holder.sol";
import "./bridge_interface.sol";

<<<<<<< HEAD
/// @title WrappedDMD ERC20 interface
=======
// Interface cho WrappedDMD token (ERC20)
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
interface IWrappedDMDToken {
    function burnWrapped(address from, uint256 amount) external;
    function mintWrapped(address to, uint256 amount) external;
    function balanceOf(address account) external view returns (uint256);
}

/**
 * @title ERC1155 Bridge Adapter
<<<<<<< HEAD
 * @dev Adapter connects ERC1155Wrapper with bridge protocol (e.g., LayerZero). Handles payload formatting, sending, and fee management.
 */
contract ERC1155BridgeAdapter is IBridgeAdapter, Ownable, Pausable, ReentrancyGuard, NonblockingLzApp, ERC1155Holder {
    // Custom Errors
=======
 * @dev Adapter kết nối ERC1155Wrapper với bridge protocol (như LayerZero)
 * Format payload, gọi hàm gửi của bridge, và handle fee
 */
contract ERC1155BridgeAdapter is IBridgeAdapter, Ownable, Pausable, ReentrancyGuard, NonblockingLzApp, ERC1155Holder {
    // Custom Errors for better gas usage and debugging
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    error ZeroAddress();
    error InvalidAmount();
    error InsufficientBalance();
    error InvalidFee();
    error ChainNotSupported(uint256 chainId);
    error BridgeNotAvailable(uint256 chainId, address bridgeAddress);
    error BridgeAlreadyExists(uint256 chainId, address bridgeAddress);
    error TransferFailed();
    error Unauthorized();
    error RateLimitExceeded();
    error WithdrawalTimelockActive();
    error StuckTokensTimelockActive();
    error InvalidTokenId();
    error FeeTooHigh(uint256 actualFee, uint256 maxFee);
    error DestinationChainPaused(uint256 chainId);

    // Địa chỉ của WrappedDMD (ERC20)
    IWrappedDMDToken public wrappedDmd;
    
    // Bridge limits
    uint256 public limitPerTx; // Giới hạn số lượng token trên mỗi giao dịch
    uint256 public limitPerPeriod; // Giới hạn số lượng token trong một khoảng thời gian
    uint256 public periodDuration = 1 days; // Thời gian của một khoảng thời gian (mặc định: 1 ngày)
    mapping(uint256 => uint256) public periodVolumes; // period => volume
    
    // Chain IDs và mapping tên chain
    uint16 private immutable _targetChain;
    uint16[] private _supportedChains;
    mapping(uint16 => bool) private _supportedChainsMap;
    uint16 private _supportedChainsCount;
    mapping(uint16 => string) public chainNames; // chainId => tên chain
    
    // Phí bridge (tính bằng basis points, 1% = 100)
    uint256 public bridgeFeePercent = 30; // 0.3% ban đầu
    
    // Địa chỉ ví nhận phí
    address public feeCollector;
    
    // Chain được hỗ trợ - dynamic mapping thay vì constants
    mapping(uint16 => bool) public supportedChains;
    
    // Trusted remotes mapping
    mapping(uint16 => bytes) public trustedRemotes; // chainId => remote address
    
    // Thời gian timelock cho các cập nhật
    uint256 public constant TIMELOCK_DURATION = 2 days;
    
    // Số lượng chữ ký tối thiểu cho multi-sig
    uint256 public constant MIN_APPROVALS = 2;
    
    // Admins for multi-sig
    mapping(address => bool) public admins;
    
    // Struct cho yêu cầu cập nhật chain
    struct ChainUpdateRequest {
        uint16 chainId;
        bool supported;
        uint256 requestTime;
        bool executed;
        mapping(address => bool) approvals; // Multi-sig approvals
        uint256 approvalCount;
    }
    
    // Mapping để theo dõi các yêu cầu cập nhật chain
    mapping(bytes32 => ChainUpdateRequest) public pendingChainUpdates;
    
    // Hợp nhất struct StuckToken, chỉ giữ một định nghĩa duy nhất
    struct StuckToken {
        address owner;
        uint256 tokenId;
        uint256 amount;
        uint16 dstChainId;
        bytes recipient;
        uint256 timestamp;
        bool rescued;
    }
    
    // Mapping để theo dõi các token bị kẹt
    mapping(bytes32 => StuckToken) public stuckTokens;
    
    // Biến để lưu trữ message thất bại cho retry
    mapping(bytes32 => FailedMessageData) private _failedMessages;
    mapping(address => bytes32[]) private _userFailedMessages;
    uint256 public maxRetryAttempts = 5;
    uint256 public retryDelay = 1 hours;
    
    // Sự kiện cho việc xử lý message thất bại
    event MessageFailed(bytes32 indexed messageId, uint16 srcChainId, address to, uint256 amount, string reason);
    event MessageRetried(bytes32 indexed messageId, bool success);
    event MessageRetryDelayUpdated(uint256 newDelay);
    event MaxRetryAttemptsUpdated(uint256 newMaxAttempts);
    
    // Struct để lưu trữ thông tin message thất bại
    struct FailedMessageData {
        uint16 srcChainId;
        bytes srcAddress;
        address to;
        uint256 amount;
        uint64 nonce;
        bytes payload;
        string reason;
        uint256 timestamp;
        uint256 retryCount;
        uint256 lastRetryTime;
        bool processed;
    }
    
    // Events
<<<<<<< HEAD
    /// @notice Emitted when a stuck token is rescued by admin/owner.
    event StuckTokenRescued(bytes32 indexed stuckId, address indexed owner, uint256 amount, address indexed rescuer, uint256 timestamp);
    /// @notice Emitted when a stuck token is registered.
    event StuckTokenRegistered(bytes32 indexed stuckId, address indexed owner, uint16 dstChainId, uint256 amount, uint256 timestamp);
=======
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    event BridgeToNear(address indexed user, bytes nearAddress, uint256 amount, uint256 fee);
    event BridgeToSolana(address indexed user, bytes solanaAddress, uint256 amount, uint256 fee);
    event TokensReceived(uint16 srcChainId, bytes srcAddress, address to, uint256 amount);
    event BridgeFeeUpdated(uint256 newFee);
    event LimitUpdated(uint256 limitPerTx, uint256 limitPerPeriod, uint256 periodDuration);
<<<<<<< HEAD
    event EmergencyWithdrawal(address indexed to, uint256 amount, address indexed actor, uint256 timestamp);
=======
    event EmergencyWithdrawal(address indexed to, uint256 amount);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    event ChainNameSet(uint16 indexed chainId, string name);
    event TrustedRemoteSet(uint16 indexed remoteChainId, bytes remoteAddress);
    event ChainUpdateRequested(bytes32 indexed updateId, uint16 indexed chainId, bool supported, uint256 unlockTime);
    event ChainUpdateApproved(bytes32 indexed updateId, address indexed approver);
    event ChainUpdateExecuted(uint16 indexed chainId, bool supported);
    event ChainUpdateCancelled(bytes32 indexed updateId);
<<<<<<< HEAD
=======
    event StuckTokenRescued(bytes32 indexed stuckId, address indexed owner, uint256 amount);
    event StuckTokenRegistered(bytes32 indexed stuckId, address indexed owner, uint16 dstChainId, uint256 amount);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    event AdminAdded(address indexed admin);
    event AdminRemoved(address indexed admin);
    
    // Bridges for different chains
    mapping(uint16 => address) public bridges;
    
    // Token address
    address public immutable tokenAddress;
    
    // Max gas amount used for bridge
    uint256 public maxGasForBridge = 3000000;
    
    // Maximum number of stuck tokens to store for a user
    uint256 public maxStuckTokensPerUser = 100;
    
    // Track stuck tokens that need to be claimed
    struct StuckToken {
        address user;
        uint256 tokenId;
        uint256 amount;
        uint16 dstChainId;
        bytes recipient;
        uint256 timestamp;
    }
    
    // Mapping from user address to their stuck tokens
    mapping(address => StuckToken[]) public userStuckTokens;
    
    // Mapping to count how many stuck tokens each user has
    mapping(address => uint256) public userStuckTokenCount;
    
    // Total stuck tokens in the system
    uint256 public totalStuckTokens;
    
    // Alert threshold - will trigger warning when reached
    uint256 public stuckTokenAlertThreshold = 1000;
    
    // Emergency mode flag
    bool public emergencyMode = false;
    
    // Thêm giới hạn tổng số stuck tokens
    uint256 public maxTotalStuckTokens = 10000;
    
    /**
     * @dev Modifier để chỉ cho phép admin thực hiện
     */
    modifier onlyAdmin() {
        require(admins[msg.sender] || msg.sender == owner(), "Caller is not admin or owner");
        _;
    }
    
    /**
     * @dev Khởi tạo contract
     * @param _wrappedDmd Địa chỉ WrappedDMD ERC20
     * @param _lzEndpoint Địa chỉ của LayerZero endpoint
     * @param targetChainId ID của chain đích chính
     * @param supportedChainIds Danh sách các chain được hỗ trợ
     * @param _limitPerTx Giới hạn số lượng token trên mỗi giao dịch
     * @param _limitPerPeriod Giới hạn số lượng token trong một khoảng thời gian
     * @param _feeCollector Địa chỉ ví nhận phí
     * @param _tokenAddress Địa chỉ của token
     */
    constructor(
        address _wrappedDmd, 
        address _lzEndpoint,
        uint16 targetChainId,
        uint16[] memory supportedChainIds,
        uint256 _limitPerTx,
        uint256 _limitPerPeriod,
        address _feeCollector,
        address _tokenAddress
    ) NonblockingLzApp(_lzEndpoint) {
        if (_wrappedDmd == address(0) || _feeCollector == address(0) || _tokenAddress == address(0)) {
            revert ZeroAddress();
        }
        
        wrappedDmd = IWrappedDMDToken(_wrappedDmd);
        _targetChain = targetChainId;
        _supportedChains = supportedChainIds;
        
        // Khởi tạo mapping từ array để tìm kiếm nhanh O(1)
        _supportedChainsCount = uint16(supportedChainIds.length);
        for (uint i = 0; i < supportedChainIds.length; i++) {
            _supportedChainsMap[supportedChainIds[i]] = true;
        }
        
        limitPerTx = _limitPerTx;
        limitPerPeriod = _limitPerPeriod;
        feeCollector = _feeCollector;
        tokenAddress = _tokenAddress;
        
        // Khởi tạo tên chain tiêu chuẩn
        _initializeChainNames();
        
        // Thêm người tạo contract vào danh sách admin
        admins[msg.sender] = true;
        emit AdminAdded(msg.sender);
    }
    
    /**
     * @dev Thêm admin mới
     * @param admin Địa chỉ admin mới
     */
    function addAdmin(address admin) external onlyOwner {
        require(admin != address(0), "Admin cannot be zero address");
        require(!admins[admin], "Already an admin");
        admins[admin] = true;
        emit AdminAdded(admin);
    }
    
    /**
     * @dev Xóa admin
     * @param admin Địa chỉ admin cần xóa
     */
    function removeAdmin(address admin) external onlyOwner {
        require(admins[admin], "Not an admin");
        admins[admin] = false;
        emit AdminRemoved(admin);
    }
    
    /**
     * @dev Khởi tạo tên chain chuẩn
     */
    function _initializeChainNames() internal {
        chainNames[1] = "Ethereum";
        chainNames[56] = "BSC";
        chainNames[137] = "Polygon";
        chainNames[43114] = "Avalanche";
        chainNames[10] = "Optimism";
        chainNames[42161] = "Arbitrum";
        chainNames[250] = "Fantom";
        chainNames[7] = "NEAR";
        chainNames[8] = "Solana";
        // Thêm các chain khác theo cần thiết
    }
    
    /**
     * @dev Lấy khoảng thời gian hiện tại
     * @return Khoảng thời gian hiện tại
     */
    function _getCurrentPeriod() internal view returns (uint256) {
        return block.timestamp / periodDuration;
    }
    
    /**
     * @dev Kiểm tra và cập nhật khối lượng trong khoảng thời gian
     * @param amount Số lượng token cần bridge
     * @return true nếu có thể bridge
     */
    function _checkAndUpdatePeriodVolume(uint256 amount) internal returns (bool) {
        uint256 currentPeriod = _getCurrentPeriod();
        uint256 newVolume = periodVolumes[currentPeriod] + amount;
        
        if (newVolume > limitPerPeriod) {
            return false;
        }
        
        periodVolumes[currentPeriod] = newVolume;
        return true;
    }
    
    /**
     * @dev Bridge WDMD token sang NEAR - Implement bridgeTo từ IBridgeAdapter
     * @param dstChainId ID của chain đích
     * @param destination Địa chỉ đích được mã hóa
     * @param amount Số lượng token cần bridge
     */
    function bridgeTo(
        uint16 dstChainId,
        bytes calldata destination,
        uint256 amount
    ) external payable override whenNotPaused nonReentrant {
        require(isChainSupported(dstChainId), "Chain not supported");
        require(trustedRemotes[dstChainId].length > 0, "Trusted remote not set for chain");
        require(amount > 0, "Amount must be greater than 0");
        require(amount <= limitPerTx, "Amount exceeds limitPerTx");
        require(destination.length > 0, "Empty destination");
        
        // Kiểm tra giới hạn số lượng token trong khoảng thời gian
        bool withinLimit = _checkAndUpdatePeriodVolume(amount);
        require(withinLimit, "Bridge volume exceeds limitPerPeriod");
        
        try this._executeBridge(dstChainId, destination, amount, msg.value, msg.sender) {
            // Bridge successful
        } catch Error(string memory reason) {
            // Log error và emit event
            emit BridgeFailed(msg.sender, dstChainId, destination, amount, reason);
            
            // Hoàn trả ETH
            (bool success, ) = msg.sender.call{value: msg.value}("");
            require(success, "ETH refund failed");
            
            // Lưu thông tin token bị kẹt
            _registerStuckToken(msg.sender, dstChainId, destination, amount);
            
            revert(reason);
        } catch (bytes memory /*lowLevelData*/) {
            // Log error cho low-level failures
            emit BridgeFailed(msg.sender, dstChainId, destination, amount, "Low-level call failed");
            
            // Hoàn trả ETH
            (bool success, ) = msg.sender.call{value: msg.value}("");
            require(success, "ETH refund failed");
            
            // Lưu thông tin token bị kẹt
            _registerStuckToken(msg.sender, dstChainId, destination, amount);
            
            revert("Bridge transaction failed");
        }
    }
    
    /**
     * @dev Lưu thông tin token bị kẹt
     * @param owner Chủ sở hữu token
     * @param dstChainId Chain đích
     * @param destination Địa chỉ đích
     * @param amount Số lượng token
     */
    function _registerStuckToken(
        address owner,
        uint16 dstChainId,
        bytes calldata destination,
        uint256 amount
    ) internal {
        bytes32 stuckId = keccak256(abi.encodePacked(
            owner,
            dstChainId,
            destination,
            amount,
            block.timestamp
        ));
        
        stuckTokens[stuckId] = StuckToken({
            owner: owner,
            dstChainId: dstChainId,
            destination: destination,
            amount: amount,
            timestamp: block.timestamp,
            rescued: false
        });
        
<<<<<<< HEAD
        emit StuckTokenRegistered(stuckId, owner, dstChainId, amount, block.timestamp);
=======
        emit StuckTokenRegistered(stuckId, owner, dstChainId, amount);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }
    
    /**
     * @dev Cứu token bị kẹt
     * @param stuckId ID của token bị kẹt
     */
<<<<<<< HEAD
    function rescueStuckToken(bytes32 stuckId) external onlyAdmin whenNotPaused {
        StuckToken storage stuck = stuckTokens[stuckId];
        require(stuck.timestamp > 0, "Stuck token not found");
        require(!stuck.rescued, "Token already rescued");
        wrappedDmd.mintWrapped(stuck.owner, stuck.amount);
        stuck.rescued = true;
        emit StuckTokenRescued(stuckId, stuck.owner, stuck.amount, msg.sender, block.timestamp);
=======
    function rescueStuckToken(bytes32 stuckId) external onlyAdmin {
        StuckToken storage stuck = stuckTokens[stuckId];
        require(stuck.timestamp > 0, "Stuck token not found");
        require(!stuck.rescued, "Token already rescued");
        
        // Mint lại token cho chủ sở hữu
        wrappedDmd.mintWrapped(stuck.owner, stuck.amount);
        
        // Đánh dấu đã cứu
        stuck.rescued = true;
        
        emit StuckTokenRescued(stuckId, stuck.owner, stuck.amount);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }
    
    /**
     * @dev Lấy danh sách token bị kẹt của một địa chỉ
     * @param owner Địa chỉ chủ sở hữu
     * @param dstChainId Chain đích
     * @return Danh sách các ID token bị kẹt
     */
    function getStuckTokens(address owner, uint16 dstChainId) external view returns (bytes32[] memory) {
        // Hàm này chỉ là mô phỏng vì Solidity không hỗ trợ dynamic array trong memory
        // Trong thực tế sẽ cần triển khai off-chain hay sử dụng cơ chế khác
    }
    
    /**
     * @dev Thực hiện bridge (được gọi từ bridgeTo)
     * @param dstChainId ID của chain đích
     * @param destination Địa chỉ đích được mã hóa
     * @param amount Số lượng token cần bridge
     * @param msgValue Số lượng ETH gửi kèm
     * @param sender Địa chỉ người gửi
     */
    function _executeBridge(
        uint16 dstChainId,
        bytes calldata destination,
        uint256 amount,
        uint256 msgValue,
        address sender
    ) external {
        require(msg.sender == address(this), "Only callable from bridgeTo");
        
        // Tính phí
        uint256 fee = (amount * bridgeFeePercent) / 10000;
        uint256 amountAfterFee = amount - fee;
        
        // Burn token
        wrappedDmd.burnWrapped(sender, amount);
        
        // Encode payload
        bytes memory payload = abi.encode(sender, destination, amountAfterFee);
        
        // Estimate fee
        (uint256 lzFee, ) = lzEndpoint.estimateFees(
            dstChainId,
            address(this),
            payload,
            false,
            bytes("")
        );
        require(msgValue >= lzFee, "Insufficient ETH for LayerZero fee");
        
        // Gửi message qua LayerZero
        _lzSend(dstChainId, payload, payable(sender), address(0), bytes(""), msgValue);
        
        emit BridgeInitiated(sender, dstChainId, destination, amountAfterFee);
        
        if (dstChainId == 7) { // NEAR
            emit BridgeToNear(sender, destination, amountAfterFee, fee);
        } else if (dstChainId == 8) { // Solana
            emit BridgeToSolana(sender, destination, amountAfterFee, fee);
        }
    }
    
    /**
     * @dev Xử lý message nhận từ LayerZero
     * @param _srcChainId Chain ID nguồn
     * @param _srcAddress Địa chỉ nguồn
     * @param _nonce Nonce
     * @param _payload Payload
     */
    function _nonblockingLzReceive(
        uint16 _srcChainId,
        bytes memory _srcAddress,
        uint64 _nonce,
        bytes memory _payload
    ) internal override {
        try {
            require(supportedChains(_srcChainId), "Chain not supported");
            
            // Kiểm tra trusted remote
            require(_srcAddress.length > 0 && trustedRemotes[_srcChainId].length > 0, "Invalid source");
            
            // So sánh bằng keccak256 thay vì so sánh từng byte
            require(keccak256(_srcAddress) == keccak256(trustedRemotes[_srcChainId]), "Source address not trusted");
            
            // Decode payload
            (address to, uint256 amount) = abi.decode(_payload, (address, uint256));
            
            // Mint token
            wrappedDmd.mintWrapped(to, amount);
            
            emit BridgeCompleted(_srcChainId, to, amount);
            emit TokensReceived(_srcChainId, _srcAddress, to, amount);
        } catch Error(string memory reason) {
            // Log error nhưng không revert để tránh stuck state
            _storeFailedMessage(_srcChainId, _srcAddress, _nonce, _payload, reason);
        }
    }
    
    /**
     * @dev Lưu trữ thông tin giao dịch thất bại để xử lý sau
     * @param _srcChainId ID của chain nguồn
     * @param _srcAddress Địa chỉ nguồn
     * @param _nonce Nonce của giao dịch
     * @param _payload Payload của giao dịch
     * @param _reason Lý do thất bại
     */
    function _storeFailedMessage(
        uint16 _srcChainId,
        bytes memory _srcAddress,
        uint64 _nonce,
        bytes memory _payload,
        string memory _reason
    ) internal {
        bytes32 messageId = keccak256(abi.encodePacked(_srcChainId, _srcAddress, _nonce));
        
        // Kiểm tra kỹ xem message có thực sự từ trusted remote không
        if (keccak256(_srcAddress) != keccak256(trustedRemotes[_srcChainId])) {
            emit MessageFailed(messageId, _srcChainId, address(0), 0, "Untrusted source address");
            return;
        }
        
        // Giải mã payload để lấy thông tin
        (address to, uint256 amount) = abi.decode(_payload, (address, uint256));
        
        // Lưu thông tin chi tiết về message thất bại
        FailedMessageData storage failedMsg = _failedMessages[messageId];
        failedMsg.srcChainId = _srcChainId;
        failedMsg.srcAddress = _srcAddress;
        failedMsg.to = to;
        failedMsg.amount = amount;
        failedMsg.nonce = _nonce;
        failedMsg.payload = _payload;
        failedMsg.reason = _reason;
        failedMsg.timestamp = block.timestamp;
        failedMsg.retryCount = 0;
        failedMsg.lastRetryTime = 0;
        failedMsg.processed = false;
        
        // Thêm messageId vào danh sách của user để dễ truy vấn
        _userFailedMessages[to].push(messageId);
        
        // Emit event để theo dõi và xử lý sau
        emit MessageFailed(messageId, _srcChainId, to, amount, _reason);
    }
    
    /**
     * @dev Thử lại xử lý message thất bại
     * @param messageId ID của message thất bại
     * @return success Kết quả xử lý
     */
    function retryFailedMessage(bytes32 messageId) external onlyAdmin returns (bool success) {
        FailedMessageData storage failedMsg = _failedMessages[messageId];
        require(failedMsg.timestamp > 0, "Message not found");
        require(!failedMsg.processed, "Message already processed");
        require(failedMsg.retryCount < maxRetryAttempts, "Max retry attempts reached");
        require(block.timestamp >= failedMsg.lastRetryTime + retryDelay, "Retry too soon");
        
        // Cập nhật thông tin retry
        failedMsg.retryCount += 1;
        failedMsg.lastRetryTime = block.timestamp;
        
        // Thử xử lý lại message
        try this._processRetryMessage(
            failedMsg.srcChainId,
            failedMsg.srcAddress,
            failedMsg.nonce,
            failedMsg.payload
        ) returns (bool result) {
            if (result) {
                failedMsg.processed = true;
                success = true;
            } else {
                success = false;
            }
        } catch {
            success = false;
        }
        
        emit MessageRetried(messageId, success);
        return success;
    }
    
    /**
     * @dev Hàm nội bộ để xử lý lại message thất bại
     * @param _srcChainId ID của chain nguồn
     * @param _srcAddress Địa chỉ nguồn
     * @param _nonce Nonce của giao dịch
     * @param _payload Payload của giao dịch
     * @return result Kết quả xử lý
     */
    function _processRetryMessage(
        uint16 _srcChainId,
        bytes memory _srcAddress,
        uint64 _nonce,
        bytes memory _payload
    ) external returns (bool result) {
        require(msg.sender == address(this), "Only callable internally");
        
        // Kiểm tra trusted remote
        require(supportedChains(_srcChainId), "Chain not supported");
        require(_srcAddress.length > 0 && trustedRemotes[_srcChainId].length > 0, "Invalid source");
        require(keccak256(_srcAddress) == keccak256(trustedRemotes[_srcChainId]), "Source address not trusted");
        
        try {
            // Decode payload
            (address to, uint256 amount) = abi.decode(_payload, (address, uint256));
            
            // Mint token
            wrappedDmd.mintWrapped(to, amount);
            
            emit BridgeCompleted(_srcChainId, to, amount);
            emit TokensReceived(_srcChainId, _srcAddress, to, amount);
            
            return true;
        } catch Error(string memory reason) {
            // Log error nhưng không revert để tránh stuck state
            bytes32 messageId = keccak256(abi.encodePacked(_srcChainId, _srcAddress, _nonce));
            emit MessageRetried(messageId, false);
            return false;
        } catch {
            bytes32 messageId = keccak256(abi.encodePacked(_srcChainId, _srcAddress, _nonce));
            emit MessageRetried(messageId, false);
            return false;
        }
    }
    
    /**
     * @dev Tự động thử lại nhiều message thất bại trong một lần gọi
     * @param limit Số lượng message tối đa để thử lại
     * @return count Số lượng message đã thử lại thành công
     */
    function batchRetryFailedMessages(uint256 limit) external onlyAdmin returns (uint256 count) {
        require(limit > 0, "Limit must be greater than 0");
        
        // Thử lại các message thất bại, ưu tiên theo thời gian và số lần retry
        uint256 successCount = 0;
        
        // Trong thực tế, bạn cần một cơ chế lưu trữ và truy vấn tất cả message thất bại
        // Đây chỉ là ví dụ, trong production sẽ cần một cơ chế hiệu quả hơn
        
        return successCount;
    }

    /**
     * @dev Lấy danh sách các message thất bại của một địa chỉ
     * @param user Địa chỉ người dùng
     * @return messageIds Danh sách ID của các message thất bại
     */
    function getUserFailedMessages(address user) external view returns (bytes32[] memory messageIds) {
        return _userFailedMessages[user];
    }
    
    /**
     * @dev Lấy thông tin chi tiết về một message thất bại
     * @param messageId ID của message
     * @return srcChainId ID của chain nguồn
     * @return to Địa chỉ nhận
     * @return amount Số lượng token
     * @return timestamp Thời điểm message thất bại
     * @return retryCount Số lần đã thử lại
     * @return lastRetryTime Thời điểm thử lại gần nhất
     * @return processed Trạng thái đã xử lý hay chưa
     * @return reason Lý do thất bại
     */
    function getFailedMessageDetails(bytes32 messageId) external view returns (
        uint16 srcChainId,
        address to,
        uint256 amount,
        uint256 timestamp,
        uint256 retryCount,
        uint256 lastRetryTime,
        bool processed,
        string memory reason
    ) {
        FailedMessageData storage failedMsg = _failedMessages[messageId];
        require(failedMsg.timestamp > 0, "Message not found");
        
        return (
            failedMsg.srcChainId,
            failedMsg.to,
            failedMsg.amount,
            failedMsg.timestamp,
            failedMsg.retryCount,
            failedMsg.lastRetryTime,
            failedMsg.processed,
            failedMsg.reason
        );
    }
    
    /**
     * @dev Cập nhật độ trễ giữa các lần thử lại
     * @param _retryDelay Độ trễ mới (giây)
     */
    function setRetryDelay(uint256 _retryDelay) external onlyOwner {
        require(_retryDelay > 0, "Retry delay must be greater than 0");
        retryDelay = _retryDelay;
        emit MessageRetryDelayUpdated(_retryDelay);
    }
    
    /**
     * @dev Cập nhật số lần thử lại tối đa
     * @param _maxRetries Số lần thử lại tối đa mới
     */
    function setMaxRetryAttempts(uint256 _maxRetries) external onlyOwner {
        require(_maxRetries > 0, "Max retries must be greater than 0");
        maxRetryAttempts = _maxRetries;
        emit MaxRetryAttemptsUpdated(_maxRetries);
    }
    
    /**
     * @dev Thủ công đánh dấu một message đã được xử lý
     * @param messageId ID của message
     */
    function markMessageAsProcessed(bytes32 messageId) external onlyAdmin {
        FailedMessageData storage failedMsg = _failedMessages[messageId];
        require(failedMsg.timestamp > 0, "Message not found");
        require(!failedMsg.processed, "Message already processed");
        
        failedMsg.processed = true;
        emit MessageRetried(messageId, true);
    }
    
    /**
     * @dev Ước tính phí
     * @param dstChainId ID của chain đích
     * @param amount Số lượng token cần bridge
     */
    function estimateFee(uint16 dstChainId, uint256 amount) external view override returns (uint256) {
        require(isChainSupported(dstChainId), "Chain not supported");
        
        // Payload mẫu
        bytes memory payload = abi.encode(address(0), bytes(""), amount);
        
        // Lấy phí từ LayerZero
        (uint256 nativeFee, ) = lzEndpoint.estimateFees(
            dstChainId,
            address(this),
            payload,
            false,
            bytes("")
        );
        
        return nativeFee;
    }
    
    /**
     * @dev Trả về danh sách các chain được hỗ trợ
     * @return Danh sách ID của các chain được hỗ trợ
     */
    function supportedChains() external view override returns (uint16[] memory) {
        return _supportedChains;
    }
    
    /**
     * @dev Trả về loại adapter
     * @return Tên loại adapter (ERC1155Bridge)
     */
    function adapterType() external pure override returns (string memory) {
        return "ERC1155Bridge";
    }
    
    /**
     * @dev Trả về ID của chain đích mà adapter này hỗ trợ
     * @return ID của chain đích
     */
    function targetChain() external view override returns (uint16) {
        return _targetChain;
    }
    
    /**
     * @dev Kiểm tra xem chain có được hỗ trợ không
     * @param chainId ID của chain cần kiểm tra
     * @return true nếu chain được hỗ trợ
     */
    function isChainSupported(uint16 chainId) public view override returns (bool) {
        return _supportedChainsMap[chainId];
    }
    
    /**
     * @dev Lấy tên của chain theo ID
     * @param chainId ID của chain cần lấy tên
     * @return Tên của chain
     */
    function getChainName(uint16 chainId) external view override returns (string memory) {
        string memory name = chainNames[chainId];
        require(bytes(name).length > 0, "Chain name not found");
        return name;
    }
    
    /**
     * @dev Cập nhật phí bridge
     * @param _newFeePercent Phí mới (basis points)
     */
    function updateBridgeFee(uint256 _newFeePercent) external onlyOwner {
        require(_newFeePercent <= 500, "Fee too high"); // Max 5%
        bridgeFeePercent = _newFeePercent;
        emit BridgeFeeUpdated(_newFeePercent);
    }
    
    /**
     * @dev Cập nhật địa chỉ nhận phí
     * @param _newCollector Địa chỉ mới
     */
    function updateFeeCollector(address _newCollector) external onlyOwner {
        require(_newCollector != address(0), "Invalid address");
        feeCollector = _newCollector;
    }
    
    /**
     * @dev Yêu cầu cập nhật chain được hỗ trợ với timelock
     * @param _chainId Chain ID
     * @param _supported Trạng thái hỗ trợ
     */
    function requestChainUpdate(uint16 _chainId, bool _supported) external onlyAdmin {
        bytes32 updateId = keccak256(abi.encodePacked(
            "CHAIN_UPDATE",
            _chainId,
            _supported,
            block.timestamp
        ));
        
        ChainUpdateRequest storage update = _getChainUpdateRequest(updateId);
        update.chainId = _chainId;
        update.supported = _supported;
        update.requestTime = block.timestamp;
        update.executed = false;
        update.approvals[msg.sender] = true;
        update.approvalCount = 1;
        
        emit ChainUpdateRequested(updateId, _chainId, _supported, block.timestamp + TIMELOCK_DURATION);
    }
    
    /**
     * @dev Phê duyệt cập nhật chain (đa chữ ký)
     * @param updateId ID của yêu cầu cập nhật
     */
    function approveChainUpdate(bytes32 updateId) external onlyAdmin {
        ChainUpdateRequest storage update = _getChainUpdateRequest(updateId);
        require(update.requestTime > 0, "Update request not found");
        require(!update.executed, "Update already executed");
        require(!update.approvals[msg.sender], "Already approved");
        
        update.approvals[msg.sender] = true;
        update.approvalCount += 1;
        
        emit ChainUpdateApproved(updateId, msg.sender);
    }
    
    /**
     * @dev Thực hiện cập nhật chain sau khi hết thời gian timelock
     * @param updateId ID của yêu cầu cập nhật
     */
    function executeChainUpdate(bytes32 updateId) external onlyAdmin {
        ChainUpdateRequest storage update = _getChainUpdateRequest(updateId);
        require(update.requestTime > 0, "Update request not found");
        require(!update.executed, "Update already executed");
        require(update.approvalCount >= MIN_APPROVALS, "Not enough approvals");
        require(block.timestamp >= update.requestTime + TIMELOCK_DURATION, "Timelock not expired");
        
        update.executed = true;
        
        if (update.supported) {
            supportedChains[update.chainId] = true;
            _supportedChainsMap[update.chainId] = true; // Cập nhật mapping
            
            // Thêm vào mảng nếu chưa có (để tương thích ngược)
            if (!_containsChainId(_supportedChains, update.chainId)) {
            _supportedChains.push(update.chainId);
        }
        
            _supportedChainsCount++;
        } else {
            supportedChains[update.chainId] = false;
            _supportedChainsMap[update.chainId] = false; // Cập nhật mapping
            
            // Xóa khỏi mảng (để tương thích ngược)
            for (uint i = 0; i < _supportedChains.length; i++) {
                if (_supportedChains[i] == update.chainId) {
                    _supportedChains[i] = _supportedChains[_supportedChains.length - 1];
                    _supportedChains.pop();
                    break;
                }
            }
            
            _supportedChainsCount--;
        }
        
        emit ChainUpdateExecuted(update.chainId, update.supported);
    }
    
    /**
     * @dev Hủy yêu cầu cập nhật chain
     * @param updateId ID của yêu cầu cập nhật
     */
    function cancelChainUpdate(bytes32 updateId) external onlyAdmin {
        ChainUpdateRequest storage update = _getChainUpdateRequest(updateId);
        require(update.requestTime > 0, "Update request not found");
        require(!update.executed, "Update already executed");
        
        emit ChainUpdateCancelled(updateId);
        
        // Reset yêu cầu cập nhật
        update.requestTime = 0;
    }
    
    /**
     * @dev Lấy struct ChainUpdateRequest từ storage
     * @param updateId ID của yêu cầu cập nhật
     * @return update ChainUpdateRequest từ storage
     */
    function _getChainUpdateRequest(bytes32 updateId) private view returns (ChainUpdateRequest storage update) {
        bytes32 position = keccak256(abi.encodePacked("ChainUpdateRequest", updateId));
        assembly {
            update.slot := position
        }
    }
    
    /**
     * @dev Cập nhật danh sách chain được hỗ trợ (phiên bản cũ, được giữ lại cho tương thích ngược)
     * @param _chainId Chain ID
     * @param _supported Trạng thái hỗ trợ
     */
    function updateSupportedChain(uint16 _chainId, bool _isSupported) external onlyOwner {
        if (_isSupported) {
            // Kiểm tra xem chain đã được hỗ trợ chưa
            if (!_supportedChainsMap[_chainId]) {
                _supportedChainsMap[_chainId] = true;
                _supportedChainsCount++;
                supportedChains[_chainId] = true;
                
                // Thêm vào mảng để tương thích ngược
                if (!_containsChainId(_supportedChains, _chainId)) {
            _supportedChains.push(_chainId);
        }
        
                emit ChainNameSet(_chainId, chainNames[_chainId]);
            }
        } else {
            // Kiểm tra xem chain đã được hỗ trợ chưa
            if (_supportedChainsMap[_chainId]) {
                _supportedChainsMap[_chainId] = false;
                _supportedChainsCount--;
                supportedChains[_chainId] = false;
                
                // Xóa khỏi mảng để tương thích ngược
            for (uint i = 0; i < _supportedChains.length; i++) {
                if (_supportedChains[i] == _chainId) {
                    _supportedChains[i] = _supportedChains[_supportedChains.length - 1];
                    _supportedChains.pop();
                    break;
                    }
                }
            }
        }
    }
    
    /**
     * @dev Kiểm tra xem một chainId có tồn tại trong mảng không
     * @param array Mảng cần kiểm tra
     * @param chainId Chain ID cần tìm
     * @return true nếu chainId tồn tại trong mảng
     */
    function _containsChainId(uint16[] memory array, uint16 chainId) internal pure returns (bool) {
        for (uint i = 0; i < array.length; i++) {
            if (array[i] == chainId) {
                return true;
            }
        }
        return false;
    }
    
    /**
     * @dev Cập nhật giới hạn bridge
     * @param _limitPerTx Giới hạn số lượng token trên mỗi giao dịch
     * @param _limitPerPeriod Giới hạn số lượng token trong một khoảng thời gian
     * @param _periodDuration Thời gian của một khoảng thời gian
     */
    function updateLimits(
        uint256 _limitPerTx,
        uint256 _limitPerPeriod,
        uint256 _periodDuration
    ) external onlyOwner {
        require(_limitPerTx > 0, "limitPerTx must be greater than 0");
        require(_limitPerPeriod > 0, "limitPerPeriod must be greater than 0");
        require(_periodDuration > 0, "periodDuration must be greater than 0");
        
        limitPerTx = _limitPerTx;
        limitPerPeriod = _limitPerPeriod;
        periodDuration = _periodDuration;
        
        emit LimitUpdated(_limitPerTx, _limitPerPeriod, _periodDuration);
    }
    
    /**
     * @dev Cài đặt trusted remote cho một chain
     * @param _srcChainId ID của chain nguồn
     * @param _srcAddress Địa chỉ nguồn được tin cậy
     */
    function setTrustedRemote(uint16 _srcChainId, bytes calldata _srcAddress) external onlyOwner {
        require(isChainSupported(_srcChainId), "Chain not supported");
        require(_srcAddress.length > 0, "Invalid source address");
        trustedRemotes[_srcChainId] = _srcAddress;
        emit TrustedRemoteSet(_srcChainId, _srcAddress);
    }
    
    /**
     * @dev Lấy trusted remote cho một chain
     * @param _srcChainId ID của chain
     * @return Địa chỉ trusted remote
     */
    function getTrustedRemote(uint16 _srcChainId) external view returns (bytes memory) {
        return trustedRemotes[_srcChainId];
    }
    
    /**
     * @dev Cập nhật tên chain
     * @param chainId ID của chain
     * @param name Tên chain
     */
    function setChainName(uint16 chainId, string calldata name) external onlyOwner {
        require(isChainSupported(chainId), "Chain not supported");
        require(bytes(name).length > 0, "Empty name");
        
        chainNames[chainId] = name;
        
        emit ChainNameSet(chainId, name);
    }
    
    /**
     * @dev Emergency withdraw - để lấy token trong trường hợp khẩn cấp
     * @param to Địa chỉ nhận token
     * @param amount Số lượng token cần withdraw
     */
<<<<<<< HEAD
    function emergencyWithdraw(address to, uint256 amount) external onlyAdmin whenNotPaused {
=======
    function emergencyWithdraw(address to, uint256 amount) external onlyAdmin {
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        require(to != address(0), "Invalid recipient");
        require(amount > 0, "Amount must be greater than 0");
        
        uint256 balance = wrappedDmd.balanceOf(address(this));
        require(balance >= amount, "Insufficient balance");
        
        // Transfer tokens
        wrappedDmd.mintWrapped(to, amount);
        
<<<<<<< HEAD
        emit EmergencyWithdrawal(to, amount, msg.sender, block.timestamp);
=======
        emit EmergencyWithdrawal(to, amount);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }
    
    /**
     * @dev Tạm dừng bridge
     */
    function pause() external onlyOwner {
        _pause();
    }
    
    /**
     * @dev Mở lại bridge
     */
    function unpause() external onlyOwner {
        _unpause();
    }
    
    /**
     * @dev Nhận ETH
     */
    receive() external payable {}

    /**
     * @dev Cleanup failed messages that are too old or have too many retry attempts
     * @param maxAge Maximum age (in seconds) of failed messages to keep
     * @param limit Maximum number of messages to clean up in one call
     * @return count Number of messages cleaned up
     */
    function cleanupOldFailedMessages(uint256 maxAge, uint256 limit) external onlyAdmin returns (uint256 count) {
        require(maxAge > 0, "Invalid max age");
        require(limit > 0 && limit <= 100, "Invalid limit");
        
        uint256 currentTime = block.timestamp;
        uint256 cutoffTime = currentTime - maxAge;
        count = 0;
        
        // Temporary storage for message IDs to delete
        bytes32[] memory messagesToCleanup = new bytes32[](limit);
        address[] memory usersToUpdate = new address[](limit);
        
        // Iterate through users to find old messages
        for (uint256 userIndex = 0; userIndex < limit; userIndex++) {
            // This assumes we have some way to iterate through users with failed messages
            address user = getAddressWithFailedMessages(userIndex);
            if (user == address(0)) break;
            
            bytes32[] memory userMessages = _userFailedMessages[user];
            for (uint256 msgIndex = 0; msgIndex < userMessages.length && count < limit; msgIndex++) {
                bytes32 messageId = userMessages[msgIndex];
                FailedMessageData storage failedMsg = _failedMessages[messageId];
                
                // Check if the message is old enough or has too many retries
                if (failedMsg.timestamp < cutoffTime || failedMsg.retryCount >= maxRetryAttempts) {
                    messagesToCleanup[count] = messageId;
                    usersToUpdate[count] = user;
                    count++;
                }
            }
        }
        
        // Process the cleanup
        for (uint256 i = 0; i < count; i++) {
            bytes32 messageId = messagesToCleanup[i];
            address user = usersToUpdate[i];
            
            // Mark as processed
            _failedMessages[messageId].processed = true;
            
            // Remove from user's list (simplified - actual implementation would need to be more robust)
            _removeFromUserFailedMessages(user, messageId);
            
            emit MessageCleaned(messageId, user);
        }
        
        return count;
    }
    
    // Helper function to simulate getting a user with failed messages
    function getAddressWithFailedMessages(uint256 index) internal view returns (address) {
        // This is a stub - in a real implementation, you'd have a way to track users with failed messages
        // This is simply to avoid compilation errors
        return address(0);
    }
    
    // Helper function to remove a message from a user's failed messages list
    function _removeFromUserFailedMessages(address user, bytes32 messageId) internal {
        bytes32[] storage userMessages = _userFailedMessages[user];
        
        for (uint256 i = 0; i < userMessages.length; i++) {
            if (userMessages[i] == messageId) {
                // Swap and pop pattern
                userMessages[i] = userMessages[userMessages.length - 1];
                userMessages.pop();
                break;
            }
        }
    }
    
    // Event for message cleanup
    event MessageCleaned(bytes32 indexed messageId, address indexed user);

    // Thêm timeout cho _executeBridge
    function _executeBridge(bytes32 stuckId) internal {
        StuckToken storage stuck = stuckTokens[stuckId];
        require(!stuck.rescued, "Already rescued");
        // Nếu stuck token quá 30 ngày thì không cho retry nữa
        require(block.timestamp <= stuck.timestamp + 30 days, "Stuck token expired");
        // ... logic xử lý bridge ...
        stuck.rescued = true;
<<<<<<< HEAD
        emit StuckTokenRescued(stuckId, stuck.owner, stuck.amount, msg.sender, block.timestamp);
=======
        emit StuckTokenRescued(stuckId, stuck.owner, stuck.amount);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        // Xóa dữ liệu stuck token cũ sau khi rescue
        delete stuckTokens[stuckId];
    }

    // Thêm hàm dọn dẹp stuck token cũ khi truy vấn
    function cleanupOldStuckTokens() external onlyAdmin {
        uint256 deleted = 0;
        for (uint256 i = 0; i < totalStuckTokens; i++) {
            bytes32 stuckId = ...; // lấy stuckId từ mapping hoặc array
            if (stuckTokens[stuckId].timestamp + 30 days < block.timestamp) {
                delete stuckTokens[stuckId];
                deleted++;
            }
            if (deleted >= 100) break; // tránh OOG
        }
    }
}
