// SPDX-License-Identifier: MIT
pragma solidity ^0.8.17;

import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import "@openzeppelin/contracts/token/ERC20/extensions/ERC20Burnable.sol";
import "@openzeppelin/contracts/access/AccessControl.sol";
import "@openzeppelin/contracts/security/Pausable.sol";
import "@layerzerolabs/solidity-examples/contracts/token/oft/v2/OFTV2.sol";

/**
 * @title DiamondToken Contract for Binance Smart Chain
 * @dev ERC20 token với bridge functionality thông qua LayerZero
 */
contract DiamondTokenBSC is ERC20, ERC20Burnable, AccessControl, Pausable, OFTV2 {
    bytes32 public constant MINTER_ROLE = keccak256("MINTER_ROLE");
    bytes32 public constant BRIDGE_ROLE = keccak256("BRIDGE_ROLE");
    bytes32 public constant PAUSER_ROLE = keccak256("PAUSER_ROLE");
    bytes32 public constant FEE_MANAGER_ROLE = keccak256("FEE_MANAGER_ROLE");
    bytes32 public constant TIER_MANAGER_ROLE = keccak256("TIER_MANAGER_ROLE");
    
    uint8 private constant _decimals = 18;
    uint256 private constant _maxSupply = 1_000_000_000 * 10**18; // 1 tỷ token
    
    // Phí bridge (chia sẻ với các token khác trong hệ sinh thái)
    uint256 public bridgeFeePercent = 50; // 0.5%
    uint256 public constant MAX_FEE_PERCENT = 1000; // 10%
    
    // Cấu trúc dữ liệu lưu trữ thông tin tier của người dùng
    mapping(address => uint8) private _userTiers;
    
    // Danh sách token được phép sử dụng theo tier
    mapping(address => uint8) private _tokenTiers;
    
    // Danh sách bridge transactions đang xử lý
    mapping(bytes32 => bool) private _pendingBridges;
    
    event BridgeFeeUpdated(uint256 oldFee, uint256 newFee);
    event BridgeInitiated(address indexed from, uint16 indexed dstChainId, bytes indexed to, uint256 amount, bytes32 transactionId);
    event BridgeCompleted(bytes32 indexed transactionId);
    
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
        _setupRole(FEE_MANAGER_ROLE, msg.sender);
        _setupRole(TIER_MANAGER_ROLE, msg.sender);
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
     * @dev Set phí bridge
     * @param newFeePercent Phí mới (tính theo phần trăm * 100, ví dụ: 50 = 0.5%)
     */
    function setBridgeFee(uint256 newFeePercent) public onlyRole(FEE_MANAGER_ROLE) {
        require(newFeePercent <= MAX_FEE_PERCENT, "DiamondToken: fee too high");
        uint256 oldFee = bridgeFeePercent;
        bridgeFeePercent = newFeePercent;
        emit BridgeFeeUpdated(oldFee, newFeePercent);
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
     * @dev Lấy tier yêu cầu của token
     * @param token Địa chỉ của token
     * @return Tier yêu cầu
     */
    function getTokenTier(address token) public view returns (uint8) {
        return _tokenTiers[token];
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
     * @dev Tính toán phí bridge
     * @param amount Số lượng token cần bridge
     * @return Số lượng token phí
     */
    function calculateBridgeFee(uint256 amount) public view returns (uint256) {
        return (amount * bridgeFeePercent) / 10000;
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
     * @param refundAddress Địa chỉ nhận phí refund
     * @param zroPaymentAddress Địa chỉ trả phí ZRO (0x0 nếu không sử dụng)
     * @param adapterParams Tham số cho adapter
     */
    function sendTokens(
        uint16 dstChainId,
        bytes memory to,
        uint256 amount,
        address payable refundAddress,
        address zroPaymentAddress,
        bytes memory adapterParams
    ) public payable {
        // Tính toán phí bridge
        uint256 fee = calculateBridgeFee(amount);
        uint256 amountAfterFee = amount - fee;
        
        // Tạo transaction ID
        bytes32 transactionId = keccak256(abi.encodePacked(msg.sender, dstChainId, to, amount, block.timestamp));
        
        // Lưu trạng thái bridge transaction
        _pendingBridges[transactionId] = true;
        
        // Emit event
        emit BridgeInitiated(msg.sender, dstChainId, to, amount, transactionId);
        
        // Send qua bridge
        _send(dstChainId, to, amountAfterFee, refundAddress, zroPaymentAddress, adapterParams);
    }
    
    /**
     * @dev Ước tính phí bridge
     * @param dstChainId Chain ID đích theo LayerZero
     * @param toAddress Địa chỉ nhận ở chain đích
     * @param amount Số lượng token
     * @param adapterParams Tham số cho adapter
     * @return Phí nativeAmount (gas fee) và zroFee
     */
    function estimateSendFee(
        uint16 dstChainId,
        bytes memory toAddress,
        uint256 amount,
        bytes memory adapterParams
    ) public view returns (uint256 nativeFee, uint256 zroFee) {
        uint256 tokenFee = calculateBridgeFee(amount);
        (uint256 lzNativeFee, uint256 lzZroFee) = _estimateSendFee(dstChainId, toAddress, amount - tokenFee, false, adapterParams);
        return (lzNativeFee, lzZroFee);
    }
    
    /**
     * @dev Đánh dấu bridge transaction đã hoàn thành
     * @param transactionId ID của transaction
     */
    function completeBridgeTransaction(bytes32 transactionId) public onlyRole(BRIDGE_ROLE) {
        require(_pendingBridges[transactionId], "DiamondToken: transaction not pending");
        _pendingBridges[transactionId] = false;
        emit BridgeCompleted(transactionId);
    }
    
    /**
     * @dev Kiểm tra xem bridge transaction có đang pending không
     * @param transactionId ID của transaction
     * @return true nếu transaction đang pending
     */
    function isBridgePending(bytes32 transactionId) public view returns (bool) {
        return _pendingBridges[transactionId];
    }
} 