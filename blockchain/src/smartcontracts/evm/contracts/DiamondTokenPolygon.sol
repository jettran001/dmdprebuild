// SPDX-License-Identifier: MIT
pragma solidity ^0.8.17;

import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import "@openzeppelin/contracts/token/ERC20/extensions/ERC20Burnable.sol";
import "@openzeppelin/contracts/access/AccessControl.sol";
import "@openzeppelin/contracts/security/Pausable.sol";
import "@layerzerolabs/solidity-examples/contracts/token/oft/v2/OFTV2.sol";

/**
 * @title DiamondToken Contract for Polygon
 * @dev ERC20 token với bridge functionality thông qua LayerZero
 */
contract DiamondTokenPolygon is ERC20, ERC20Burnable, AccessControl, Pausable, OFTV2 {
    bytes32 public constant MINTER_ROLE = keccak256("MINTER_ROLE");
    bytes32 public constant BRIDGE_ROLE = keccak256("BRIDGE_ROLE");
    bytes32 public constant PAUSER_ROLE = keccak256("PAUSER_ROLE");
    bytes32 public constant TIER_MANAGER_ROLE = keccak256("TIER_MANAGER_ROLE");
    bytes32 public constant EMERGENCY_ROLE = keccak256("EMERGENCY_ROLE");
    
    uint8 private constant _decimals = 18;
    uint256 private constant _maxSupply = 1_000_000_000 * 10**18; // 1 tỷ token
    
    // Cấu trúc dữ liệu lưu trữ thông tin tier của người dùng
    mapping(address => uint8) private _userTiers;
    
    // Danh sách token được phép sử dụng theo tier
    mapping(address => uint8) private _tokenTiers;
    
    // Lưu trữ các bridge request
    struct BridgeRequest {
        address sender;
        uint16 dstChainId;
        bytes recipient;
        uint256 amount;
        uint256 timestamp;
        bool completed;
    }
    
    // ID giao dịch bridge đến thông tin request
    mapping(bytes32 => BridgeRequest) private _bridgeRequests;
    
    // Danh sách bridge requests theo người dùng
    mapping(address => bytes32[]) private _userBridgeRequests;
    
    // Bridge cooldown time (giây)
    uint256 public bridgeCooldown = 300; // 5 phút
    
    // Cấu hình gas tiêu hao cho bridge
    mapping(uint16 => uint256) private _bridgeGasLimits;
    
    event BridgeRequested(bytes32 indexed requestId, address indexed sender, uint16 dstChainId, bytes recipient, uint256 amount);
    event BridgeCompleted(bytes32 indexed requestId);
    event EmergencyWithdraw(address indexed token, address indexed to, uint256 amount);
    
    /**
     * @dev Khởi tạo contract với tham số ban đầu
     * @param name Tên token
     * @param symbol Ký hiệu token
     * @param lzEndpoint Địa chỉ của LayerZero Endpoint
     */
    constructor(
        string memory name,
        string memory symbol,
        address lzEndpoint
    ) ERC20(name, symbol) OFTV2(name, symbol, _decimals, lzEndpoint) {
        _setupRole(DEFAULT_ADMIN_ROLE, msg.sender);
        _setupRole(MINTER_ROLE, msg.sender);
        _setupRole(BRIDGE_ROLE, msg.sender);
        _setupRole(PAUSER_ROLE, msg.sender);
        _setupRole(TIER_MANAGER_ROLE, msg.sender);
        _setupRole(EMERGENCY_ROLE, msg.sender);
        
        // Thiết lập gas limit mặc định cho các chain phổ biến
        _bridgeGasLimits[1] = 200000; // Ethereum
        _bridgeGasLimits[56] = 150000; // BSC
        _bridgeGasLimits[42161] = 300000; // Arbitrum
        _bridgeGasLimits[43114] = 200000; // Avalanche
    }
    
    /**
     * @dev Trả về số decimals của token
     */
    function decimals() public pure override(ERC20, OFTV2) returns (uint8) {
        return _decimals;
    }
    
    /**
     * @dev Tạo token mới, chỉ có thể được gọi bởi address có MINTER_ROLE
     * @param to Địa chỉ nhận token
     * @param amount Số lượng token tạo
     */
    function mint(address to, uint256 amount) public onlyRole(MINTER_ROLE) {
        require(totalSupply() + amount <= _maxSupply, "DiamondToken: exceeds max supply");
        _mint(to, amount);
    }
    
    /**
     * @dev Tạm dừng chuyển token
     */
    function pause() public onlyRole(PAUSER_ROLE) {
        _pause();
    }
    
    /**
     * @dev Tiếp tục chuyển token sau khi tạm dừng
     */
    function unpause() public onlyRole(PAUSER_ROLE) {
        _unpause();
    }
    
    /**
     * @dev Set bridge cooldown
     * @param newCooldown Thời gian cooldown mới (giây)
     */
    function setBridgeCooldown(uint256 newCooldown) public onlyRole(BRIDGE_ROLE) {
        bridgeCooldown = newCooldown;
    }
    
    /**
     * @dev Set gas limit cho một chain
     * @param chainId Chain ID
     * @param gasLimit Gas limit mới
     */
    function setBridgeGasLimit(uint16 chainId, uint256 gasLimit) public onlyRole(BRIDGE_ROLE) {
        _bridgeGasLimits[chainId] = gasLimit;
    }
    
    /**
     * @dev Set tier cho người dùng
     * @param user Địa chỉ của người dùng
     * @param tier Tier mới (0-5)
     */
    function setUserTier(address user, uint8 tier) public onlyRole(TIER_MANAGER_ROLE) {
        require(tier <= 5, "DiamondToken: invalid tier value");
        _userTiers[user] = tier;
    }
    
    /**
     * @dev Batch set tier cho nhiều người dùng
     * @param users Danh sách địa chỉ của người dùng
     * @param tiers Danh sách tier mới
     */
    function batchSetUserTiers(address[] memory users, uint8[] memory tiers) public onlyRole(TIER_MANAGER_ROLE) {
        require(users.length == tiers.length, "DiamondToken: array length mismatch");
        for (uint256 i = 0; i < users.length; i++) {
            require(tiers[i] <= 5, "DiamondToken: invalid tier value");
            _userTiers[users[i]] = tiers[i];
        }
    }
    
    /**
     * @dev Set tier cho token
     * @param token Địa chỉ của token
     * @param requiredTier Tier yêu cầu tối thiểu để sử dụng token
     */
    function setTokenTier(address token, uint8 requiredTier) public onlyRole(TIER_MANAGER_ROLE) {
        require(requiredTier <= 5, "DiamondToken: invalid tier value");
        _tokenTiers[token] = requiredTier;
    }
    
    /**
     * @dev Lấy tier của người dùng
     * @param user Địa chỉ của người dùng
     * @return Tier của người dùng
     */
    function getUserTier(address user) public view returns (uint8) {
        return _userTiers[user];
    }
    
    /**
     * @dev Lấy thông tin bridge request
     * @param requestId ID của request
     * @return BridgeRequest struct
     */
    function getBridgeRequest(bytes32 requestId) public view returns (
        address sender,
        uint16 dstChainId,
        bytes memory recipient,
        uint256 amount,
        uint256 timestamp,
        bool completed
    ) {
        BridgeRequest memory request = _bridgeRequests[requestId];
        return (
            request.sender,
            request.dstChainId,
            request.recipient,
            request.amount,
            request.timestamp,
            request.completed
        );
    }
    
    /**
     * @dev Lấy danh sách bridge requests của người dùng
     * @param user Địa chỉ của người dùng
     * @return Danh sách request IDs
     */
    function getUserBridgeRequests(address user) public view returns (bytes32[] memory) {
        return _userBridgeRequests[user];
    }
    
    /**
     * @dev Kiểm tra xem token có được phép sử dụng bởi người dùng không
     * @param token Địa chỉ của token
     * @param user Địa chỉ của người dùng
     * @return true nếu token được phép sử dụng
     */
    function isTokenAllowedForUser(address token, address user) public view returns (bool) {
        return _userTiers[user] >= _tokenTiers[token];
    }
    
    /**
     * @dev Trước khi chuyển token, kiểm tra xem sender có tier phù hợp không
     */
    function _beforeTokenTransfer(
        address from,
        address to,
        uint256 amount
    ) internal override whenNotPaused {
        super._beforeTokenTransfer(from, to, amount);
    }
    
    /**
     * @dev Send token qua bridge đến chain khác
     * @param dstChainId Chain ID đích theo LayerZero
     * @param to Địa chỉ nhận ở chain đích
     * @param amount Số lượng token
     */
    function bridgeTokens(
        uint16 dstChainId,
        bytes memory to,
        uint256 amount
    ) public payable {
        // Kiểm tra cooldown
        bytes32[] memory userRequests = _userBridgeRequests[msg.sender];
        if (userRequests.length > 0) {
            bytes32 lastRequestId = userRequests[userRequests.length - 1];
            BridgeRequest memory lastRequest = _bridgeRequests[lastRequestId];
            require(
                block.timestamp >= lastRequest.timestamp + bridgeCooldown,
                "DiamondToken: bridge cooldown active"
            );
        }
        
        // Gas limit cho chain đích
        uint256 gasLimit = _bridgeGasLimits[dstChainId];
        require(gasLimit > 0, "DiamondToken: unsupported destination chain");
        
        // Tạo adapter params với gas limit
        bytes memory adapterParams = abi.encodePacked(uint16(1), gasLimit);
        
        // Tạo bridge request ID
        bytes32 requestId = keccak256(abi.encodePacked(
            msg.sender,
            dstChainId,
            to,
            amount,
            block.timestamp
        ));
        
        // Lưu bridge request
        _bridgeRequests[requestId] = BridgeRequest({
            sender: msg.sender,
            dstChainId: dstChainId,
            recipient: to,
            amount: amount,
            timestamp: block.timestamp,
            completed: false
        });
        
        // Thêm vào danh sách request của user
        _userBridgeRequests[msg.sender].push(requestId);
        
        // Emit event
        emit BridgeRequested(requestId, msg.sender, dstChainId, to, amount);
        
        // Thực hiện bridge
        _send(
            dstChainId,
            to,
            amount,
            payable(msg.sender), // refundAddress
            address(0), // zroPaymentAddress
            adapterParams
        );
    }
    
    /**
     * @dev Đánh dấu bridge request hoàn thành
     * @param requestId ID của request
     */
    function completeBridgeRequest(bytes32 requestId) public onlyRole(BRIDGE_ROLE) {
        require(_bridgeRequests[requestId].sender != address(0), "DiamondToken: request not found");
        require(!_bridgeRequests[requestId].completed, "DiamondToken: already completed");
        
        _bridgeRequests[requestId].completed = true;
        emit BridgeCompleted(requestId);
    }
    
    /**
     * @dev Ước tính phí bridge
     * @param dstChainId Chain ID đích theo LayerZero
     * @param toAddress Địa chỉ nhận ở chain đích
     * @param amount Số lượng token
     * @return Phí nativeAmount (gas fee) và zroFee
     */
    function estimateBridgeFee(
        uint16 dstChainId,
        bytes memory toAddress,
        uint256 amount
    ) public view returns (uint256 nativeFee, uint256 zroFee) {
        // Gas limit cho chain đích
        uint256 gasLimit = _bridgeGasLimits[dstChainId];
        require(gasLimit > 0, "DiamondToken: unsupported destination chain");
        
        // Tạo adapter params với gas limit
        bytes memory adapterParams = abi.encodePacked(uint16(1), gasLimit);
        
        return _estimateSendFee(dstChainId, toAddress, amount, false, adapterParams);
    }
    
    /**
     * @dev Rút token khẩn cấp trong trường hợp có vấn đề
     * @param token Địa chỉ token cần rút (address(0) cho native token)
     * @param to Địa chỉ nhận
     * @param amount Số lượng
     */
    function emergencyWithdraw(
        address token,
        address to,
        uint256 amount
    ) public onlyRole(EMERGENCY_ROLE) {
        if (token == address(0)) {
            // Native token (MATIC)
            payable(to).transfer(amount);
        } else {
            // ERC20 token
            IERC20(token).transfer(to, amount);
        }
        
        emit EmergencyWithdraw(token, to, amount);
    }
} 