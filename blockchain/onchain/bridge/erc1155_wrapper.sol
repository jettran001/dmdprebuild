// SPDX-License-Identifier: MIT
pragma solidity 0.8.20;

import "@openzeppelin/contracts/token/ERC1155/IERC1155.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/security/Pausable.sol";
import "@openzeppelin/contracts/token/ERC1155/utils/ERC1155Holder.sol";
import "@openzeppelin/contracts/security/ReentrancyGuard.sol";
import "@openzeppelin/contracts/utils/Address.sol";
import "@openzeppelin/contracts/token/ERC20/IERC20.sol";

/**
 * @title IWrappedDMD
<<<<<<< HEAD
 * @dev Interface for WrappedDMD token
=======
 * @dev Interface cho WrappedDMD token
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
 */
interface IWrappedDMD {
    function mint(address to, uint256 amount) external;
    function burnFrom(address account, uint256 amount) external;
    function balanceOf(address account) external view returns (uint256);
    function approve(address spender, uint256 amount) external returns (bool);
    function allowance(address owner, address spender) external view returns (uint256);
}

/**
 * @title IBridgeInterface
<<<<<<< HEAD
 * @dev Interface for Bridge contract
=======
 * @dev Interface cho Bridge contract
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
 */
interface IBridgeInterface {
    function bridge(uint16 dstChainId, bytes calldata toAddress, uint256 amount) external payable;
    function bridgeAndUnwrap(uint16 dstChainId, bytes calldata toAddress, uint256 amount, address wrapper) external payable;
    function estimateFee(uint16 dstChainId, bytes calldata toAddress, uint256 amount) external view returns (uint256, uint256);
    function isTokenSupported(address token) external view returns (bool);
    function getTrustedRemote(uint16 chainId) external view returns (bytes memory);
    function hasTrustedRemote(uint16 chainId) external view returns (bool);
}

/**
 * @title ERC1155Wrapper
<<<<<<< HEAD
 * @dev Contract to wrap DMD ERC-1155 to DMD ERC-20 and vice versa. Part of the bridge workflow.
=======
 * @dev Contract wrap DMD ERC-1155 thành DMD ERC-20 và ngược lại
 * Là một phần quan trọng trong luồng hoạt động của bridge
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
 * DMD ERC-1155 → wrap → DMD ERC-20 → LayerZero → DMD NEAR
 * DMD NEAR → LayerZero → DMD ERC-20 → unwrap → DMD ERC-1155
 */
contract ERC1155Wrapper is ERC1155Holder, Ownable, Pausable, ReentrancyGuard {
    using Address for address;
    
    // Custom Errors
    error ZeroAddress();
    error InvalidAmount();
    error InsufficientBalance();
    error TransferFailed();
    error NotApproved();
    error InvalidToken();
    error Paused();
    error RateLimitExceeded();
    error BridgeAddressNotSet();
    error TokenApprovalFailed();
    error UnsupportedToken();
    error ChainNotSupported();
    
    // Events
<<<<<<< HEAD
    /// @notice Emitted when tokens are wrapped.
    event Wrapped(address indexed user, uint256 amount, uint256 timestamp);
    /// @notice Emitted when tokens are unwrapped.
    event Unwrapped(address indexed user, uint256 amount, uint256 timestamp);
    event TokenAddressUpdated(address indexed oldToken, address indexed newToken);
    event WrappedTokenAddressUpdated(address indexed oldToken, address indexed newToken);
    /// @notice Emitted when the bridge address is updated.
    event BridgeAddressUpdated(address indexed oldBridge, address indexed newBridge, address indexed updater, uint256 timestamp);
    /// @notice Emitted when wrap and bridge is initiated.
    event WrapAndBridgeInitiated(address indexed user, uint16 dstChainId, bytes destination, uint256 amount, uint256 timestamp);
    /// @notice Emitted when unwrapping fails.
    event UnwrapFailed(address indexed user, uint256 amount, string reason, uint256 timestamp);
    /// @notice Emitted when token approval is updated.
    event TokenApproved(address indexed token, address indexed spender, uint256 amount, uint256 timestamp);
    /// @notice Emitted when wrap rate limits are updated.
    event RateLimitUpdated(uint256 newDailyLimit, uint256 newUserLimit, address indexed updater, uint256 timestamp);
    /// @notice Emitted when stuck ERC1155 tokens are rescued.
    event StuckERC1155Rescued(address indexed token, uint256 tokenId, uint256 amount, address indexed rescuer, uint256 timestamp);
    /// @notice Emitted when stuck ERC20 tokens are rescued.
    event StuckERC20Rescued(address indexed token, uint256 amount, address indexed rescuer, uint256 timestamp);
    /// @notice Emitted when stuck ETH is rescued.
    event StuckETHRescued(address indexed to, uint256 amount, address indexed rescuer, uint256 timestamp);
=======
    event Wrapped(address indexed user, uint256 amount);
    event Unwrapped(address indexed user, uint256 amount);
    event TokenAddressUpdated(address indexed oldToken, address indexed newToken);
    event WrappedTokenAddressUpdated(address indexed oldToken, address indexed newToken);
    event BridgeAddressUpdated(address indexed oldBridge, address indexed newBridge);
    event WrapAndBridgeInitiated(address indexed user, uint16 dstChainId, bytes destination, uint256 amount);
    event UnwrapFailed(address indexed user, uint256 amount, string reason);
    event TokenApproved(address indexed token, address indexed spender, uint256 amount);
    event RateLimitUpdated(uint256 newDailyLimit, uint256 newUserLimit);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    
    // State variables
    IERC1155 public diamondToken;
    IWrappedDMD public wrappedDMDToken;
    uint256 public constant DMD_TOKEN_ID = 0; // DMD token ID
    
    // Bridge interface address
    address public bridgeInterface;
    
    // Tracking của số lượng token đã wrapped/unwrapped
    mapping(address => uint256) public userWrappedAmount;
    mapping(address => uint256) public userUnwrappedAmount;
    uint256 public totalWrappedAmount;
    uint256 public totalUnwrappedAmount;
    
    // Rate limiting
    uint256 public dailyWrapLimit = 1000000 ether; // Giới hạn số lượng token wrap mỗi ngày
    uint256 public userWrapLimit = 10000 ether; // Giới hạn số lượng token wrap mỗi user trong một ngày
    mapping(uint256 => uint256) public dailyWrapVolume; // day => volume
    mapping(address => mapping(uint256 => uint256)) public userDailyWrapVolume; // user => day => volume
    
    // Cơ chế phục hồi giao dịch thất bại
    struct FailedUnwrap {
        address user;
        uint256 amount;
        uint256 timestamp;
        string reason;
        bool processed;
    }
    
    // Lưu trữ các giao dịch unwrap thất bại
    mapping(bytes32 => FailedUnwrap) public failedUnwraps;
    
    /**
     * @dev Constructor
     * @param _diamondToken Địa chỉ token ERC-1155
     * @param _wrappedDMDToken Địa chỉ token WrappedDMD ERC-20
     */
    constructor(address _diamondToken, address _wrappedDMDToken) Ownable(msg.sender) {
        if (_diamondToken == address(0)) revert ZeroAddress();
        if (_wrappedDMDToken == address(0)) revert ZeroAddress();
        
        diamondToken = IERC1155(_diamondToken);
        wrappedDMDToken = IWrappedDMD(_wrappedDMDToken);
    }
    
    /**
     * @dev Lấy ngày hiện tại (Unix timestamp chia cho số giây trong 1 ngày)
     * @return Ngày hiện tại
     */
    function _getCurrentDay() internal view returns (uint256) {
        return block.timestamp / 1 days;
    }
    
    /**
     * @dev Kiểm tra và cập nhật khối lượng wrap trong ngày
     * @param user Địa chỉ người dùng
     * @param amount Số lượng token
     * @return Có vượt quá giới hạn không
     */
    function _checkAndUpdateWrapVolume(address user, uint256 amount) internal returns (bool) {
        uint256 currentDay = _getCurrentDay();
        
        // Kiểm tra giới hạn toàn cục
        uint256 newDailyVolume = dailyWrapVolume[currentDay] + amount;
        if (newDailyVolume > dailyWrapLimit) {
            return false;
        }
        
        // Kiểm tra giới hạn của user
        uint256 newUserVolume = userDailyWrapVolume[user][currentDay] + amount;
        if (newUserVolume > userWrapLimit) {
            return false;
        }
        
        // Cập nhật volume
        dailyWrapVolume[currentDay] = newDailyVolume;
        userDailyWrapVolume[user][currentDay] = newUserVolume;
        
        return true;
    }
    
    /**
     * @dev Đảm bảo bridge đã được approve để sử dụng wrapped token
     * @param amount Số lượng token cần approve
     */
    function _ensureBridgeApproval(uint256 amount) internal {
        if (bridgeInterface == address(0)) revert BridgeAddressNotSet();
        
        uint256 currentAllowance = wrappedDMDToken.allowance(address(this), bridgeInterface);
        if (currentAllowance < amount) {
            bool success = wrappedDMDToken.approve(bridgeInterface, type(uint256).max);
            if (!success) revert TokenApprovalFailed();
            
<<<<<<< HEAD
            emit TokenApproved(address(wrappedDMDToken), bridgeInterface, type(uint256).max, block.timestamp);
=======
            emit TokenApproved(address(wrappedDMDToken), bridgeInterface, type(uint256).max);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        }
    }
    
    /**
     * @dev Kiểm tra xem bridge có hỗ trợ token không
     * @return Có hỗ trợ không
     */
    function _isBridgeSupportingToken() internal view returns (bool) {
        if (bridgeInterface == address(0)) return false;
        
        try IBridgeInterface(bridgeInterface).isTokenSupported(address(wrappedDMDToken)) returns (bool supported) {
            return supported;
        } catch {
            return false;
        }
    }
    
    /**
     * @dev Kiểm tra xem bridge có trusted remote cho chain đích không
     * @param dstChainId ID của chain đích
     * @return Có trusted remote không
     */
    function _hasTrustedRemote(uint16 dstChainId) internal view returns (bool) {
        if (bridgeInterface == address(0)) return false;
        
        try IBridgeInterface(bridgeInterface).hasTrustedRemote(dstChainId) returns (bool hasTrusted) {
            return hasTrusted;
        } catch {
            return false;
        }
    }
    
    /**
     * @dev Wrap DMD ERC-1155 thành DMD ERC-20
     * @param _amount Số lượng token cần wrap
     */
    function wrap(uint256 _amount) external whenNotPaused nonReentrant {
        if (_amount == 0) revert InvalidAmount();
        if (diamondToken.balanceOf(msg.sender, DMD_TOKEN_ID) < _amount) revert InsufficientBalance();
        
        // Kiểm tra rate limit
        bool withinLimit = _checkAndUpdateWrapVolume(msg.sender, _amount);
        if (!withinLimit) revert RateLimitExceeded();
        
        // Kiểm tra approval
        if (!diamondToken.isApprovedForAll(msg.sender, address(this))) revert NotApproved();
        
        // Transfer DMD từ user đến contract (CEI Pattern)
        diamondToken.safeTransferFrom(msg.sender, address(this), DMD_TOKEN_ID, _amount, "");
        
        // Mint Wrapped DMD cho user
        wrappedDMDToken.mint(msg.sender, _amount);
        
        // Cập nhật tracking
        userWrappedAmount[msg.sender] += _amount;
        totalWrappedAmount += _amount;
        
<<<<<<< HEAD
        emit Wrapped(msg.sender, _amount, block.timestamp);
=======
        emit Wrapped(msg.sender, _amount);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }
    
    /**
     * @dev Unwrap DMD ERC-20 thành DMD ERC-1155
     * @param _amount Số lượng token cần unwrap
     */
    function unwrap(uint256 _amount) external whenNotPaused nonReentrant {
        if (_amount == 0) revert InvalidAmount();
        if (wrappedDMDToken.balanceOf(msg.sender) < _amount) revert InsufficientBalance();
        
        // Burn Wrapped DMD từ user (CEI Pattern)
        wrappedDMDToken.burnFrom(msg.sender, _amount);
        
        // Cập nhật tracking trước khi transfer token
        userUnwrappedAmount[msg.sender] += _amount;
        totalUnwrappedAmount += _amount;
        
        // Transfer DMD từ contract đến user
        try diamondToken.safeTransferFrom(address(this), msg.sender, DMD_TOKEN_ID, _amount, "") {
<<<<<<< HEAD
            emit Unwrapped(msg.sender, _amount, block.timestamp);
=======
            emit Unwrapped(msg.sender, _amount);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        } catch Error(string memory reason) {
            // Lưu thông tin unwrap thất bại
            bytes32 failedId = keccak256(abi.encodePacked(msg.sender, _amount, block.timestamp));
            failedUnwraps[failedId] = FailedUnwrap({
                user: msg.sender,
                amount: _amount,
                timestamp: block.timestamp,
                reason: reason,
                processed: false
            });
            
<<<<<<< HEAD
            emit UnwrapFailed(msg.sender, _amount, reason, block.timestamp);
=======
            emit UnwrapFailed(msg.sender, _amount, reason);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        } catch (bytes memory) {
            // Lưu thông tin unwrap thất bại với lý do chung
            bytes32 failedId = keccak256(abi.encodePacked(msg.sender, _amount, block.timestamp));
            failedUnwraps[failedId] = FailedUnwrap({
                user: msg.sender,
                amount: _amount,
                timestamp: block.timestamp,
                reason: "Unknown error",
                processed: false
            });
            
<<<<<<< HEAD
            emit UnwrapFailed(msg.sender, _amount, "Unknown error", block.timestamp);
=======
            emit UnwrapFailed(msg.sender, _amount, "Unknown error");
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        }
    }
    
    /**
     * @dev Wrap DMD ERC-1155 và bridge đến chain khác trong một giao dịch
     * @param _amount Số lượng token cần wrap và bridge
     * @param _dstChainId Chain ID đích
     * @param _toAddress Địa chỉ nhận được mã hóa
     */
    function wrapAndBridge(uint256 _amount, uint16 _dstChainId, bytes calldata _toAddress) external payable whenNotPaused nonReentrant {
        if (_amount == 0) revert InvalidAmount();
        if (diamondToken.balanceOf(msg.sender, DMD_TOKEN_ID) < _amount) revert InsufficientBalance();
        if (bridgeInterface == address(0)) revert BridgeAddressNotSet();
        
        // Kiểm tra bridge có hỗ trợ token và chain
        if (!_isBridgeSupportingToken()) revert UnsupportedToken();
        if (!_hasTrustedRemote(_dstChainId)) revert ChainNotSupported();
        
        // Kiểm tra rate limit
        bool withinLimit = _checkAndUpdateWrapVolume(msg.sender, _amount);
        if (!withinLimit) revert RateLimitExceeded();
        
        // Kiểm tra approval
        if (!diamondToken.isApprovedForAll(msg.sender, address(this))) revert NotApproved();
        
        // Transfer DMD từ user đến contract (CEI Pattern)
        diamondToken.safeTransferFrom(msg.sender, address(this), DMD_TOKEN_ID, _amount, "");
        
        // Mint Wrapped DMD cho contract này
        wrappedDMDToken.mint(address(this), _amount);
        
        // Cập nhật tracking
        userWrappedAmount[msg.sender] += _amount;
        totalWrappedAmount += _amount;
        
<<<<<<< HEAD
        emit Wrapped(msg.sender, _amount, block.timestamp);
=======
        emit Wrapped(msg.sender, _amount);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        
        // Đảm bảo bridge đã được approve
        _ensureBridgeApproval(_amount);
        
        // Bridge token đến chain đích
        IBridgeInterface(bridgeInterface).bridge{value: msg.value}(_dstChainId, _toAddress, _amount);
        
<<<<<<< HEAD
        emit WrapAndBridgeInitiated(msg.sender, _dstChainId, _toAddress, _amount, block.timestamp);
=======
        emit WrapAndBridgeInitiated(msg.sender, _dstChainId, _toAddress, _amount);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }
    
    /**
     * @dev Wrap, bridge và unwrap tại chain đích (nếu đích là EVM)
     * @param _amount Số lượng token cần wrap và bridge
     * @param _dstChainId Chain ID đích
     * @param _toAddress Địa chỉ nhận được mã hóa
     */
    function wrapAndBridgeWithUnwrap(uint256 _amount, uint16 _dstChainId, bytes calldata _toAddress) external payable whenNotPaused nonReentrant {
        if (_amount == 0) revert InvalidAmount();
        if (diamondToken.balanceOf(msg.sender, DMD_TOKEN_ID) < _amount) revert InsufficientBalance();
        if (bridgeInterface == address(0)) revert BridgeAddressNotSet();
        
        // Kiểm tra bridge có hỗ trợ token và chain
        if (!_isBridgeSupportingToken()) revert UnsupportedToken();
        if (!_hasTrustedRemote(_dstChainId)) revert ChainNotSupported();
        
        // Kiểm tra rate limit
        bool withinLimit = _checkAndUpdateWrapVolume(msg.sender, _amount);
        if (!withinLimit) revert RateLimitExceeded();
        
        // Kiểm tra approval
        if (!diamondToken.isApprovedForAll(msg.sender, address(this))) revert NotApproved();
        
        // Transfer DMD từ user đến contract (CEI Pattern)
        diamondToken.safeTransferFrom(msg.sender, address(this), DMD_TOKEN_ID, _amount, "");
        
        // Mint Wrapped DMD cho contract này
        wrappedDMDToken.mint(address(this), _amount);
        
        // Cập nhật tracking
        userWrappedAmount[msg.sender] += _amount;
        totalWrappedAmount += _amount;
        
<<<<<<< HEAD
        emit Wrapped(msg.sender, _amount, block.timestamp);
=======
        emit Wrapped(msg.sender, _amount);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        
        // Đảm bảo bridge đã được approve
        _ensureBridgeApproval(_amount);
        
        // Bridge token đến chain đích với unwrap tự động
        IBridgeInterface(bridgeInterface).bridgeAndUnwrap{value: msg.value}(_dstChainId, _toAddress, _amount, address(this));
        
<<<<<<< HEAD
        emit WrapAndBridgeInitiated(msg.sender, _dstChainId, _toAddress, _amount, block.timestamp);
=======
        emit WrapAndBridgeInitiated(msg.sender, _dstChainId, _toAddress, _amount);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }
    
    /**
     * @dev Chuẩn bị dữ liệu cho bridge
     * @param _dstChainId Chain ID đích
     * @param _toAddress Địa chỉ nhận được mã hóa
     * @return Dữ liệu đã chuẩn bị
     */
    function prepareForBridge(uint16 _dstChainId, bytes calldata _toAddress) external view returns (bytes memory) {
        if (bridgeInterface == address(0)) revert BridgeAddressNotSet();
        
        // Định dạng dữ liệu theo yêu cầu của bridge
        return abi.encode(_dstChainId, _toAddress);
    }
    
    /**
     * @dev Ước tính phí bridge
     * @param _dstChainId Chain ID đích
     * @param _toAddress Địa chỉ nhận được mã hóa
     * @param _amount Số lượng token cần bridge
     * @return nativeFee Phí native token
     * @return zroFee Phí ZRO token (nếu có)
     */
    function estimateBridgeFee(uint16 _dstChainId, bytes calldata _toAddress, uint256 _amount) external view returns (uint256 nativeFee, uint256 zroFee) {
        if (bridgeInterface == address(0)) revert BridgeAddressNotSet();
        
        return IBridgeInterface(bridgeInterface).estimateFee(_dstChainId, _toAddress, _amount);
    }
    
    /**
     * @dev Lấy thông tin bridge
     * @param _dstChainId Chain ID đích
     * @return isSupportedToken Bridge có hỗ trợ token không
     * @return hasTrustedRemote Bridge có trusted remote cho chain đích không
     * @return trustedRemote Địa chỉ trusted remote (nếu có)
     */
    function getBridgeInfo(uint16 _dstChainId) external view returns (bool isSupportedToken, bool hasTrustedRemote, bytes memory trustedRemote) {
        if (bridgeInterface == address(0)) return (false, false, bytes(""));
        
        isSupportedToken = _isBridgeSupportingToken();
        hasTrustedRemote = _hasTrustedRemote(_dstChainId);
        
        if (hasTrustedRemote) {
            try IBridgeInterface(bridgeInterface).getTrustedRemote(_dstChainId) returns (bytes memory remote) {
                trustedRemote = remote;
            } catch {
                trustedRemote = bytes("");
            }
        }
        
        return (isSupportedToken, hasTrustedRemote, trustedRemote);
    }
    
    /**
     * @dev Lấy số lượng token đã wrapped và unwrapped của một địa chỉ
     * @param _user Địa chỉ cần kiểm tra
     * @return wrapped Số lượng token đã wrapped
     * @return unwrapped Số lượng token đã unwrapped
     */
    function getUserWrappedStats(address _user) external view returns (uint256 wrapped, uint256 unwrapped) {
        return (userWrappedAmount[_user], userUnwrappedAmount[_user]);
    }
    
    /**
     * @dev Thử lại unwrap token cho giao dịch thất bại
     * @param failedId ID của giao dịch thất bại
     */
    function retryFailedUnwrap(bytes32 failedId) external onlyOwner {
        FailedUnwrap storage failedTx = failedUnwraps[failedId];
        
        if (failedTx.user == address(0)) revert InvalidToken();
        if (failedTx.processed) revert InvalidAmount();
        
        try diamondToken.safeTransferFrom(address(this), failedTx.user, DMD_TOKEN_ID, failedTx.amount, "") {
            failedTx.processed = true;
<<<<<<< HEAD
            emit Unwrapped(failedTx.user, failedTx.amount, block.timestamp);
        } catch Error(string memory reason) {
            failedTx.reason = reason;
            emit UnwrapFailed(failedTx.user, failedTx.amount, reason, block.timestamp);
        } catch (bytes memory) {
            failedTx.reason = "Unknown error on retry";
            emit UnwrapFailed(failedTx.user, failedTx.amount, "Unknown error on retry", block.timestamp);
=======
            emit Unwrapped(failedTx.user, failedTx.amount);
        } catch Error(string memory reason) {
            failedTx.reason = reason;
            emit UnwrapFailed(failedTx.user, failedTx.amount, reason);
        } catch (bytes memory) {
            failedTx.reason = "Unknown error on retry";
            emit UnwrapFailed(failedTx.user, failedTx.amount, "Unknown error on retry");
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
        }
    }
    
    /**
     * @dev Lấy thông tin giám sát unwrap
     * @param user Địa chỉ người dùng
     * @return wrapped Số lượng đã wrap
     * @return unwrapped Số lượng đã unwrap
     * @return balance Số dư ERC-1155 trong contract
     */
    function getUnwrapMonitoringInfo(address user) external view returns (uint256 wrapped, uint256 unwrapped, uint256 balance) {
        wrapped = userWrappedAmount[user];
        unwrapped = userUnwrappedAmount[user];
        balance = diamondToken.balanceOf(address(this), DMD_TOKEN_ID);
        return (wrapped, unwrapped, balance);
    }
    
    /**
     * @dev Cập nhật địa chỉ token ERC-1155
     * @param _newToken Địa chỉ token mới
     */
    function updateDiamondTokenAddress(address _newToken) external onlyOwner {
        if (_newToken == address(0)) revert ZeroAddress();
        
        address oldToken = address(diamondToken);
        diamondToken = IERC1155(_newToken);
        
        emit TokenAddressUpdated(oldToken, _newToken);
    }
    
    /**
     * @dev Cập nhật địa chỉ token Wrapped DMD
     * @param _newToken Địa chỉ token mới
     */
<<<<<<< HEAD
    function setWrappedTokenForChain(uint16 chainId, address token) external onlyOwner {
        require(token != address(0), "Zero address");
        // Gọi qua bridge_interface hoặc trực tiếp ChainRegistry nếu dùng chung storage
        // ChainRegistry.setWrappedToken(chainRegistry, chainId, token);
        // Hoặc nếu dùng qua bridge_interface:
        DiamondBridgeInterface(bridgeInterface).setWrappedTokenForChain(chainId, token);
        emit WrappedTokenAddressUpdated(address(0), token); // Cập nhật event cho đúng logic
=======
    function updateWrappedTokenAddress(address _newToken) external onlyOwner {
        if (_newToken == address(0)) revert ZeroAddress();
        
        address oldToken = address(wrappedDMDToken);
        wrappedDMDToken = IWrappedDMD(_newToken);
        
        emit WrappedTokenAddressUpdated(oldToken, _newToken);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }
    
    /**
     * @dev Cập nhật địa chỉ bridge interface
     * @param _newBridge Địa chỉ bridge mới
     */
    function updateBridgeAddress(address _newBridge) external onlyOwner {
        if (_newBridge == address(0)) revert ZeroAddress();
        
        address oldBridge = bridgeInterface;
        bridgeInterface = _newBridge;
        
        // Cấp quyền cho bridge mới
        wrappedDMDToken.approve(_newBridge, type(uint256).max);
        
<<<<<<< HEAD
        emit BridgeAddressUpdated(oldBridge, _newBridge, msg.sender, block.timestamp);
        emit TokenApproved(address(wrappedDMDToken), _newBridge, type(uint256).max, block.timestamp);
=======
        emit BridgeAddressUpdated(oldBridge, _newBridge);
        emit TokenApproved(address(wrappedDMDToken), _newBridge, type(uint256).max);
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }
    
    /**
     * @dev Cập nhật giới hạn wrap
     * @param _dailyLimit Giới hạn số lượng token wrap mỗi ngày
     * @param _userLimit Giới hạn số lượng token wrap mỗi user trong một ngày
     */
    function updateWrapLimits(uint256 _dailyLimit, uint256 _userLimit) external onlyOwner {
        if (_dailyLimit == 0 || _userLimit == 0) revert InvalidAmount();
        if (_userLimit > _dailyLimit) revert InvalidAmount();
        
        dailyWrapLimit = _dailyLimit;
        userWrapLimit = _userLimit;
        
<<<<<<< HEAD
        emit RateLimitUpdated(_dailyLimit, _userLimit, msg.sender, block.timestamp);
    }
    
    /// @notice Rescue stuck ERC1155 tokens from the contract. Only callable by the owner.
    function rescueStuckERC1155(address token, uint256 tokenId, uint256 amount) external onlyOwner whenNotPaused {
        if (token == address(0)) revert ZeroAddress();
        if (amount == 0) revert InvalidAmount();
        IERC1155(token).safeTransferFrom(address(this), owner(), tokenId, amount, "");
        emit StuckERC1155Rescued(token, tokenId, amount, msg.sender, block.timestamp);
    }
    
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
=======
        emit RateLimitUpdated(_dailyLimit, _userLimit);
    }
    
    /**
     * @dev Lấy lại token khẩn cấp bị mắc kẹt
     * @param _token Địa chỉ token
     * @param _tokenId Token ID (cần thiết cho ERC-1155)
     * @param _amount Số lượng token
     */
    function rescueStuckTokens(address _token, uint256 _tokenId, uint256 _amount) external onlyOwner {
        if (_token == address(0)) revert ZeroAddress();
        if (_amount == 0) revert InvalidAmount();
        
        IERC1155(_token).safeTransferFrom(address(this), owner(), _tokenId, _amount, "");
    }
    
    /**
     * @dev Lấy lại token ERC20 bị mắc kẹt
     * @param _token Địa chỉ token
     * @param _amount Số lượng token
     */
    function rescueStuckERC20(address _token, uint256 _amount) external onlyOwner {
        if (_token == address(0)) revert ZeroAddress();
        if (_amount == 0) revert InvalidAmount();
        
        IERC20(_token).transfer(owner(), _amount);
    }
    
    /**
     * @dev Lấy lại ETH bị mắc kẹt
     * @param _amount Số lượng ETH
     */
    function rescueStuckETH(uint256 _amount) external onlyOwner {
        if (_amount == 0) revert InvalidAmount();
        if (address(this).balance < _amount) revert InsufficientBalance();
        
        (bool success, ) = owner().call{value: _amount}("");
        if (!success) revert TransferFailed();
>>>>>>> 9885b0ca0cc72ab80191a810cbab6f59610c4c39
    }
    
    /**
     * @dev Pause contract
     */
    function pause() external onlyOwner {
        _pause();
    }
    
    /**
     * @dev Unpause contract
     */
    function unpause() external onlyOwner {
        _unpause();
    }
    
    /**
     * @dev Nhận ETH
     */
    receive() external payable {}
}
