// SPDX-License-Identifier: MIT
pragma solidity 0.8.20;

import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/security/Pausable.sol";
import "@openzeppelin/contracts/security/ReentrancyGuard.sol";

<<<<<<< HEAD
/// @title DiamondToken ERC1155 interface
=======
// Interface cho DiamondToken ERC1155
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
interface IDiamondToken {
    function unwrapFromERC20(address to, uint256 amount) external;
    function balanceOf(address account, uint256 id) external view returns (uint256);
}

<<<<<<< HEAD
/// @title Bridge interface
interface IBridgeInterface {
    function bridgeToken(
        uint16 dstChainId,
=======
// Interface cho Bridge
interface IBridgeInterface {
    function bridgeToken(
        uint16 dstChainId, 
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        bytes memory toAddress,
        uint256 amount,
        address token,
        bool needUnwrap
    ) external payable;
}

/**
 * @title Wrapped DMD Token (wDMD)
<<<<<<< HEAD
 * @dev ERC20 wrapper token for DiamondToken (DMD) ERC1155, used as an intermediate token for bridging between ERC1155 and NEAR
 */
contract WrappedDMD is ERC20, Ownable, Pausable, ReentrancyGuard {
    // Custom Errors
=======
 * @dev ERC20 wrapper token cho DiamondToken (DMD) ERC1155
 * Được sử dụng làm token trung gian để bridge giữa ERC1155 và NEAR
 */
contract WrappedDMD is ERC20, Ownable, Pausable, ReentrancyGuard {
    // Custom Errors - recommended by .solguard for better gas optimization
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    error ZeroAddress();
    error Unauthorized();
    error InsufficientBalance();
    error InvalidAmount();
    error BridgeDisabled();
    error ProxyNotVerified();
    error BridgeOperationFailed(string reason);
    error RateLimitExceeded();
    error BlacklistedRemote();
    error TimelockNotExpired();
    error EmergencyShutdown();
    error TransactionLimitExceeded();
    error MaxMintersExceeded();
    error MaxBurnersExceeded();
    error EmergencyLimitExceeded();
    error AddressNotAuthorized();
    error ProxyAlreadyVerified();
    error PermissionDenied();
<<<<<<< HEAD
    error TransferFailed();

    IDiamondToken public dmdToken;
    address public bridgeProxy;
    uint256 public constant DMD_TOKEN_ID = 0;
    mapping(address => uint256) public userNonces;
    uint256 public maxBridgeAmountPerDay = 1000000 ether;
    uint256 public bridgeCooldown = 1 hours;
    mapping(uint256 => uint256) public dailyBridgeVolume;
    mapping(address => uint256) public lastBridgeTimestamp;

=======
    
    // Địa chỉ của DiamondToken ERC1155
    IDiamondToken public dmdToken;
    
    // Địa chỉ của bridge proxy
    address public bridgeProxy;
    
    // ID của DMD token trong ERC1155
    uint256 public constant DMD_TOKEN_ID = 0;
    
    // Nonce để ngăn chặn front-running
    mapping(address => uint256) public userNonces;
    
    // Rate limit settings
    uint256 public maxBridgeAmountPerDay = 1000000 ether;
    uint256 public bridgeCooldown = 1 hours;
    
    // Rate limit tracking
    mapping(uint256 => uint256) public dailyBridgeVolume; // day => volume
    mapping(address => uint256) public lastBridgeTimestamp; // user => last bridge timestamp
    
    // Bridge request tracking
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    struct BridgeRequest {
        address sender;
        bytes destination;
        uint256 amount;
        uint256 timestamp;
        bool completed;
        bool refunded;
        string status;
    }
<<<<<<< HEAD
    mapping(bytes32 => BridgeRequest) public bridgeRequests;
    mapping(uint256 => bytes32[]) public bridgesByTimestamp;

    /// @notice Emitted when tokens are wrapped
    event TokenWrapped(address indexed from, uint256 amount, uint256 timestamp);
    /// @notice Emitted when tokens are unwrapped
    event TokenUnwrapped(address indexed to, uint256 amount, uint256 timestamp);
    /// @notice Emitted when a bridge request is created
    event BridgeRequested(bytes32 indexed bridgeId, address indexed sender, bytes destination, uint256 amount, uint256 timestamp);
    /// @notice Emitted when a bridge request is completed
    event BridgeCompleted(bytes32 indexed bridgeId, address indexed sender, bytes destination, uint256 amount, uint256 timestamp);
    /// @notice Emitted when a bridge request fails
    event BridgeFailed(bytes32 indexed bridgeId, address indexed sender, bytes destination, uint256 amount, string reason, uint256 timestamp);
=======
    
    mapping(bytes32 => BridgeRequest) public bridgeRequests;
    
    // Theo dõi bridge request theo thời gian
    mapping(uint256 => bytes32[]) public bridgesByTimestamp; // day timestamp => bridge IDs
    
    // Events
    event TokenWrapped(address indexed from, uint256 amount);
    event TokenUnwrapped(address indexed to, uint256 amount);
    event BridgeRequested(bytes32 indexed bridgeId, address indexed sender, bytes destination, uint256 amount);
    event BridgeCompleted(bytes32 indexed bridgeId, address indexed sender, bytes destination, uint256 amount);
    event BridgeFailed(bytes32 indexed bridgeId, address indexed sender, bytes destination, uint256 amount, string reason);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    event BridgeProxySet(address indexed oldProxy, address indexed newProxy);
    event BridgeProxyPermissionUpdated(bool mintingEnabled, bool burningEnabled);
    event DMDTokenSet(address indexed token);
    event TransactionLimitsSet(uint256 maxMintPerTransaction, uint256 maxBurnPerTransaction);
    event TrustedRemoteBlacklisted(bytes remote, bool blacklisted);
    event EmergencyProxyDisabled();
    event MinterAdded(address indexed minter);
    event MinterRemoved(address indexed minter);
    event BurnerAdded(address indexed burner);
    event BurnerRemoved(address indexed burner);
    event EmergencyMintLimitUpdated(uint256 newLimit);
    event AuthorizedBurnerUpdated(address indexed burner, address indexed target, bool authorized);
<<<<<<< HEAD
    event TokenWrappedAndBridged(address indexed from, uint16 indexed dstChainId, bytes indexed toAddress, uint256 amount, bytes32 bridgeId, bool needUnwrap, uint256 timestamp);
    event PermissionCacheSynced(bool mintingEnabled, bool burningEnabled);
    /// @notice Emitted when ERC20 tokens are rescued from the contract by the owner.
    event StuckERC20Rescued(address indexed token, uint256 amount, address indexed rescuer, uint256 timestamp);
    /// @notice Emitted when ETH is rescued from the contract by the owner.
    event StuckETHRescued(address indexed to, uint256 amount, address indexed rescuer, uint256 timestamp);

=======
    event TokenWrappedAndBridged(address indexed from, uint16 indexed dstChainId, bytes indexed toAddress, uint256 amount, bytes32 bridgeId, bool needUnwrap);
    event PermissionCacheSynced(bool mintingEnabled, bool burningEnabled);
    
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    // --- New variables for enhanced security ---
    
    // Proxy permission controls
    bool public proxyMintingEnabled = false;
    bool public proxyBurningEnabled = false;
    
    // Transaction limits
    uint256 public maxMintPerTransaction = 1000000 ether;
    uint256 public maxBurnPerTransaction = 1000000 ether;
    
    // Bridge proxy verification
    mapping(address => bool) public verifiedProxies;
    
    // Blacklisted trusted remotes
    mapping(bytes => bool) public blacklistedTrustedRemotes;
    
    // Emergency timelock
    uint256 public constant EMERGENCY_TIMELOCK = 2 days;
    
    // Proxy change request
    struct ProxyChangeRequest {
        address newProxy;
        uint256 requestTime;
        bool executed;
    }
    
    ProxyChangeRequest public pendingProxyChange;

    // Bug #10 Fix: Add more security measures for proxy and role management
    
    // Minters and burners list and limits
    address[] public minters;
    address[] public burners;
    mapping(address => bool) public isMinter;
    mapping(address => bool) public isBurner;
    uint8 public mintersCount; // Count of minters
    uint8 public burnersCount; // Count of burners
    uint8 public constant MAX_MINTERS = 5; // Maximum number of minters
    uint8 public constant MAX_BURNERS = 5; // Maximum number of burners
    
    // Emergency mint limit
    uint256 public emergencyMintLimit = 10000000 ether; // Default 10M tokens
    uint256 public emergencyMintUsed; // Amount of emergency mint used
    
    // Authorized burners for specific addresses
    mapping(address => mapping(address => bool)) public authorizedBurners; // burner => target => authorized

    /**
     * @dev Khởi tạo Wrapped DMD token
     * @param name Tên token
     * @param symbol Symbol token
     * @param _dmdToken Địa chỉ của DiamondToken
     */
    constructor(string memory name, string memory symbol, address _dmdToken) ERC20(name, symbol) {
        if (_dmdToken == address(0)) revert ZeroAddress();
        dmdToken = IDiamondToken(_dmdToken);
        emit DMDTokenSet(_dmdToken);
    }
    
    /**
     * @dev Modifier để kiểm tra caller là DMD token hoặc bridge proxy
     */
    modifier onlyAuthorized() {
        if (msg.sender != address(dmdToken) && msg.sender != bridgeProxy) 
            revert Unauthorized();
        _;
    }
    
    /**
     * @dev Modifier để kiểm tra proxy đã được xác minh
     */
    modifier onlyVerifiedProxy() {
        if (!verifiedProxies[msg.sender]) revert ProxyNotVerified();
        _;
    }
    
    /**
     * @dev Modifier to check if caller is a minter
     */
    modifier onlyMinter() {
        if (!isMinter[msg.sender] && msg.sender != owner()) revert Unauthorized();
        _;
    }
    
    /**
     * @dev Modifier to check if caller is a burner
     */
    modifier onlyBurner() {
        if (!isBurner[msg.sender] && msg.sender != owner()) revert Unauthorized();
        _;
    }
    
    /**
     * @dev Tạo wrapped token từ DMD ERC1155
     * @param to Địa chỉ nhận token
     * @param amount Số lượng token
     */
    function mintWrapped(address to, uint256 amount) external nonReentrant whenNotPaused {
        // Check if caller is dmdToken or verified proxy
        if (msg.sender == bridgeProxy) {
            // Bridge proxy minting check
            if (!proxyMintingEnabled) revert BridgeDisabled();
            if (amount > maxMintPerTransaction) revert TransactionLimitExceeded();
        } else if (msg.sender != address(dmdToken) && !isMinter[msg.sender] && msg.sender != owner()) {
            revert Unauthorized();
        }
        
        // If emergency minting (by owner or minter), check emergency limit
        if (msg.sender != address(dmdToken) && msg.sender != bridgeProxy) {
            if (emergencyMintUsed + amount > emergencyMintLimit) revert EmergencyLimitExceeded();
            emergencyMintUsed += amount;
        }
        
        // Mint token to specified address
        _mint(to, amount);
        
<<<<<<< HEAD
        emit TokenWrapped(to, amount, block.timestamp);
=======
        emit TokenWrapped(to, amount);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }
    
    /**
     * @dev Burn wrapped token để unwrap về DMD ERC1155
     * @param from Địa chỉ burn token
     * @param amount Số lượng token
     */
    function burnWrapped(address from, uint256 amount) external nonReentrant whenNotPaused {
        // Check if caller is dmdToken or verified proxy
        if (msg.sender == bridgeProxy) {
            // Bridge proxy burning check
            if (!proxyBurningEnabled) revert BridgeDisabled();
            if (amount > maxBurnPerTransaction) revert TransactionLimitExceeded();
        } else if (msg.sender != address(dmdToken) && !isBurner[msg.sender] && msg.sender != owner()) {
            revert Unauthorized();
        }
        
        // If burning from someone else's address, check authorization
        if (msg.sender != from && msg.sender != owner() && msg.sender != address(dmdToken)) {
            // Check if burner is authorized for this specific address
            if (!authorizedBurners[msg.sender][from]) revert AddressNotAuthorized();
        }
        
        // Check balance
        if (balanceOf(from) < amount) revert InsufficientBalance();
        
        // Burn token
        _burn(from, amount);
        
<<<<<<< HEAD
        emit TokenUnwrapped(from, amount, block.timestamp);
=======
        emit TokenUnwrapped(from, amount);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }
    
    /**
     * @dev Unwrap from ERC20 to ERC1155
     * @param amount Số lượng token cần unwrap
     */
    function unwrap(uint256 amount) external nonReentrant whenNotPaused {
        if (amount == 0) revert InvalidAmount();
        if (balanceOf(msg.sender) < amount) revert InsufficientBalance();
        
        // Burn wrapped token first (follow CEI pattern)
        _burn(msg.sender, amount);
        
        // Call to DMD token to mint ERC1155
        dmdToken.unwrapFromERC20(msg.sender, amount);
        
<<<<<<< HEAD
        emit TokenUnwrapped(msg.sender, amount, block.timestamp);
=======
        emit TokenUnwrapped(msg.sender, amount);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }
    
    /**
     * @dev Set bridge proxy address with timelock protection
     * @param _proxy Địa chỉ bridge proxy mới
     */
    function requestProxyChange(address _proxy) external onlyOwner {
        if (_proxy == address(0)) revert ZeroAddress();
        
        // Create a new proxy change request with timelock
        pendingProxyChange = ProxyChangeRequest({
            newProxy: _proxy,
            requestTime: block.timestamp,
            executed: false
        });
    }
    
    /**
     * @dev Verify if a bridge proxy is verified and has appropriate permissions
     * @param operation 1 for mint, 2 for burn
     * @return bool True if proxy can perform the operation
     */
    function verifyProxyOperation(uint8 operation) external view returns (bool) {
        // Sửa lỗi: Chỉ cho phép proxy hoặc owner truy vấn
        if (msg.sender != bridgeProxy && msg.sender != owner()) {
            return false;
        }
        
        // Kiểm tra proxy có được xác minh không
        if (!verifiedProxies[msg.sender] && msg.sender != owner()) {
            return false;
        }
        
        // Kiểm tra quyền tương ứng
        if (operation == 1) return proxyMintingEnabled;
        if (operation == 2) return proxyBurningEnabled;
        return false;
    }
    
    /**
     * @dev Execute pending proxy change after timelock period
     */
    function executeProxyChange() external onlyOwner {
        if (pendingProxyChange.requestTime == 0) revert BridgeOperationFailed("No pending change");
        if (pendingProxyChange.executed) revert BridgeOperationFailed("Already executed");
        if (block.timestamp < pendingProxyChange.requestTime + EMERGENCY_TIMELOCK) revert TimelockNotExpired();
        
        address oldProxy = bridgeProxy;
        address newProxy = pendingProxyChange.newProxy;
        
        // Sửa lỗi: Nếu proxy mới đã được xác minh thì cảnh báo và không thực hiện lại
        if (verifiedProxies[newProxy]) {
            revert ProxyAlreadyVerified();
        }
        
        // Reset permissions for old proxy if exists
        if (oldProxy != address(0)) {
            verifiedProxies[oldProxy] = false;
        }
        
        // Update bridge proxy
        bridgeProxy = newProxy;
        verifiedProxies[newProxy] = true;
        
        // Sửa lỗi: Đồng bộ cache quyền
        proxyMintingEnabled = true;
        proxyBurningEnabled = true;
        
        // Mark as executed
        pendingProxyChange.executed = true;
        
        emit BridgeProxySet(oldProxy, newProxy);
        emit BridgeProxyPermissionUpdated(proxyMintingEnabled, proxyBurningEnabled);
        emit PermissionCacheSynced(proxyMintingEnabled, proxyBurningEnabled);
    }
    
    /**
     * @dev Set proxy minting/burning permission (chỉ owner)
     */
    function setProxyPermission(bool minting, bool burning) external onlyOwner {
        bool oldMintingEnabled = proxyMintingEnabled;
        bool oldBurningEnabled = proxyBurningEnabled;
        
        proxyMintingEnabled = minting;
        proxyBurningEnabled = burning;
        
        // Emit event only if permissions changed
        if (oldMintingEnabled != minting || oldBurningEnabled != burning) {
            emit BridgeProxyPermissionUpdated(minting, burning);
        }
    }
    
    /**
     * @dev Set transaction limits
     * @param _maxMint Maximum tokens per mint transaction
     * @param _maxBurn Maximum tokens per burn transaction
     */
    function setTransactionLimits(uint256 _maxMint, uint256 _maxBurn) external onlyOwner {
        if (_maxMint == 0 || _maxBurn == 0) revert InvalidAmount();
        
        maxMintPerTransaction = _maxMint;
        maxBurnPerTransaction = _maxBurn;
        
        emit TransactionLimitsSet(_maxMint, _maxBurn);
    }
    
    /**
     * @dev Blacklist a trusted remote
     * @param remote Remote address bytes
     * @param blacklisted Blacklist status
     */
    function setTrustedRemoteBlacklist(bytes calldata remote, bool blacklisted) external onlyOwner {
        if (remote.length == 0) revert InvalidAmount();
        
        blacklistedTrustedRemotes[remote] = blacklisted;
        
        emit TrustedRemoteBlacklisted(remote, blacklisted);
    }
    
    /**
     * @dev Set DiamondToken address (only for migration)
     * @param _dmdToken Address of DiamondToken
     */
    function setDMDToken(address _dmdToken) external onlyOwner {
        if (_dmdToken == address(0)) revert ZeroAddress();
        dmdToken = IDiamondToken(_dmdToken);
        emit DMDTokenSet(_dmdToken);
    }
    
    /**
     * @dev Pause token operations
     */
    function pause() external onlyOwner {
        _pause();
    }
    
    /**
     * @dev Unpause token operations
     */
    function unpause() external onlyOwner {
        _unpause();
    }
    
    /**
     * @dev Add a new minter address
     * @param minter Address to add as minter
     */
    function addMinter(address minter) external onlyOwner {
        if (minter == address(0)) revert ZeroAddress();
        if (isMinter[minter]) revert BridgeOperationFailed("Already a minter");
        if (mintersCount >= MAX_MINTERS) revert MaxMintersExceeded();
        
        isMinter[minter] = true;
        minters.push(minter);
        mintersCount++;
        
        emit MinterAdded(minter);
    }
    
    /**
     * @dev Remove a minter address
     * @param minter Address to remove as minter
     */
    function removeMinter(address minter) external onlyOwner {
        if (!isMinter[minter]) revert BridgeOperationFailed("Not a minter");
        
        isMinter[minter] = false;
        // Find and remove from array
        for (uint8 i = 0; i < minters.length; i++) {
            if (minters[i] == minter) {
                minters[i] = minters[minters.length - 1];
                minters.pop();
                break;
            }
        }
        mintersCount--;
        
        emit MinterRemoved(minter);
    }
    
    /**
     * @dev Add a new burner address
     * @param burner Address to add as burner
     */
    function addBurner(address burner) external onlyOwner {
        if (burner == address(0)) revert ZeroAddress();
        if (isBurner[burner]) revert BridgeOperationFailed("Already a burner");
        if (burnersCount >= MAX_BURNERS) revert MaxBurnersExceeded();
        
        isBurner[burner] = true;
        burners.push(burner);
        burnersCount++;
        
        emit BurnerAdded(burner);
    }
    
    /**
     * @dev Remove a burner address
     * @param burner Address to remove as burner
     */
    function removeBurner(address burner) external onlyOwner {
        if (!isBurner[burner]) revert BridgeOperationFailed("Not a burner");
        
        isBurner[burner] = false;
        // Find and remove from array
        for (uint8 i = 0; i < burners.length; i++) {
            if (burners[i] == burner) {
                burners[i] = burners[burners.length - 1];
                burners.pop();
                break;
            }
        }
        burnersCount--;
        
        emit BurnerRemoved(burner);
    }
    
    /**
     * @dev Update emergency mint limit
     * @param _limit New emergency mint limit
     */
    function setEmergencyMintLimit(uint256 _limit) external onlyOwner {
        if (_limit == 0) revert InvalidAmount();
        
        emergencyMintLimit = _limit;
        
        emit EmergencyMintLimitUpdated(_limit);
    }
    
    /**
     * @dev Reset emergency mint used counter (for recovery after emergency)
     */
    function resetEmergencyMintUsed() external onlyOwner {
        emergencyMintUsed = 0;
    }
    
    /**
     * @dev Authorize a burner to burn tokens from a specific address
     * @param burner The burner address
     * @param target The target address whose tokens can be burned
     * @param authorized Whether the burner is authorized
     */
    function setAuthorizedBurner(address burner, address target, bool authorized) external onlyOwner {
        if (burner == address(0) || target == address(0)) revert ZeroAddress();
        
        authorizedBurners[burner][target] = authorized;
        
        emit AuthorizedBurnerUpdated(burner, target, authorized);
    }
    
    /**
     * @dev Get list of all minters
     * @return Array of minter addresses
     */
    function getAllMinters() external view returns (address[] memory) {
        return minters;
    }
    
    /**
     * @dev Get list of all burners
     * @return Array of burner addresses
     */
    function getAllBurners() external view returns (address[] memory) {
        return burners;
    }
    
    /**
     * @dev Check if a burner is authorized for a specific address
     * @param burner The burner address
     * @param target The target address
     * @return Whether the burner is authorized
     */
    function isAuthorizedBurner(address burner, address target) external view returns (bool) {
        return authorizedBurners[burner][target];
    }
    
    /**
     * @dev Wrap and bridge tokens to another chain in a single transaction
     * @param amount Amount of tokens to wrap and bridge
     * @param dstChainId Destination chain ID
     * @param toAddress Address on destination chain (encoded)
     * @param needUnwrap Whether to automatically unwrap on destination
     * @return bridgeId Unique identifier for the bridge operation
     */
    function wrap_to_chain(
        uint256 amount, 
        uint16 dstChainId, 
        bytes calldata toAddress,
        bool needUnwrap
    ) external payable whenNotPaused nonReentrant returns (bytes32 bridgeId) {
        if (amount == 0) revert InvalidAmount();
        if (bridgeProxy == address(0)) revert BridgeOperationFailed("Bridge proxy not set");
        if (!verifiedProxies[bridgeProxy]) revert ProxyNotVerified();
        
        // Kiểm tra giới hạn giao dịch
        if (amount > maxMintPerTransaction) revert TransactionLimitExceeded();
        
        // Kiểm tra giới hạn thời gian
        if (block.timestamp - lastBridgeTimestamp[msg.sender] < bridgeCooldown) {
            revert RateLimitExceeded();
        }
        
        // Kiểm tra giới hạn khối lượng hàng ngày
        uint256 currentDay = block.timestamp / 1 days;
        if (dailyBridgeVolume[currentDay] + amount > maxBridgeAmountPerDay) {
            revert RateLimitExceeded();
        }
        
        // Kiểm tra destination không nằm trong blacklist
        if (blacklistedTrustedRemotes[toAddress]) {
            revert BlacklistedRemote();
        }
        
        // Cập nhật rate limit tracking
        dailyBridgeVolume[currentDay] += amount;
        lastBridgeTimestamp[msg.sender] = block.timestamp;
        
        // Tăng nonce user để tránh front-running
        userNonces[msg.sender]++;
        
        // First mint wrapped tokens to this contract
        _mint(address(this), amount);
        
        // Approve bridge proxy to spend tokens
        _approve(address(this), bridgeProxy, amount);
        
        // Generate bridge ID with nonce để cải thiện tính unique
        bridgeId = keccak256(abi.encode(
            msg.sender,
            dstChainId,
            toAddress,
            amount,
            block.timestamp,
            block.number,
            userNonces[msg.sender]
        ));
        
        // Lưu thông tin bridge request
        bridgeRequests[bridgeId] = BridgeRequest({
            sender: msg.sender,
            destination: toAddress,
            amount: amount,
            timestamp: block.timestamp,
            completed: false,
            refunded: false,
            status: "Initiated"
        });
        
        // Thêm bridge ID vào danh sách theo ngày
        bridgesByTimestamp[currentDay].push(bridgeId);
        
        // Call bridge function on proxy
        try IBridgeInterface(bridgeProxy).bridgeToken{value: msg.value}(
            dstChainId,
            toAddress,
            amount,
            address(this),
            needUnwrap
        ) {
            // Cập nhật trạng thái bridge request
            bridgeRequests[bridgeId].status = "Completed";
            bridgeRequests[bridgeId].completed = true;
            
            emit TokenWrappedAndBridged(
                msg.sender,
                dstChainId,
                toAddress,
                amount,
                bridgeId,
<<<<<<< HEAD
                needUnwrap,
                block.timestamp
            );
            emit BridgeCompleted(bridgeId, msg.sender, toAddress, amount, block.timestamp);
=======
                needUnwrap
            );
            emit BridgeCompleted(bridgeId, msg.sender, toAddress, amount);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        } catch Error(string memory reason) {
            // If bridge fails, burn the tokens to avoid stuck tokens
            _burn(address(this), amount);
            
            // Cập nhật trạng thái bridge request
            bridgeRequests[bridgeId].status = reason;
            bridgeRequests[bridgeId].refunded = true;
            
<<<<<<< HEAD
            emit BridgeFailed(bridgeId, msg.sender, toAddress, amount, reason, block.timestamp);
=======
            emit BridgeFailed(bridgeId, msg.sender, toAddress, amount, reason);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
            revert BridgeOperationFailed(reason);
        } catch {
            // If bridge fails with no reason, burn the tokens
            _burn(address(this), amount);
            
            // Cập nhật trạng thái bridge request
            bridgeRequests[bridgeId].status = "Unknown error";
            bridgeRequests[bridgeId].refunded = true;
            
<<<<<<< HEAD
            emit BridgeFailed(bridgeId, msg.sender, toAddress, amount, "Bridge operation failed", block.timestamp);
=======
            emit BridgeFailed(bridgeId, msg.sender, toAddress, amount, "Bridge operation failed");
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
            revert BridgeOperationFailed("Bridge operation failed");
        }
        
        return bridgeId;
    }
<<<<<<< HEAD

    /// @notice Rescue stuck ERC20 tokens from the contract. Only callable by the owner.
    function rescueStuckERC20(address token, uint256 amount) external onlyOwner whenNotPaused {
        if (token == address(0)) revert ZeroAddress();
        if (amount == 0) revert InvalidAmount();
        IERC20(token).transfer(owner(), amount);
        emit StuckERC20Rescued(token, amount, msg.sender, block.timestamp);
    }

    /// @notice Rescue stuck ETH from the contract. Only callable by the owner.
    function rescueStuckETH(uint256 amount) external onlyOwner whenNotPaused {
        if (amount == 0) revert InvalidAmount();
        if (address(this).balance < amount) revert InsufficientBalance();
        address to = owner();
        (bool success, ) = to.call{value: amount}("");
        if (!success) revert TransferFailed();
        emit StuckETHRescued(to, amount, msg.sender, block.timestamp);
    }
=======
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
}
