// SPDX-License-Identifier: MIT
pragma solidity ^0.8.17;

import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import "@openzeppelin/contracts/token/ERC20/extensions/ERC20Burnable.sol";
import "@openzeppelin/contracts/access/AccessControl.sol";
import "@openzeppelin/contracts/security/Pausable.sol";
import "@layerzerolabs/solidity-examples/contracts/token/oft/v2/OFTV2.sol";

/**
 * @title DiamondToken Contract for Ethereum
 * @dev ERC20 token với bridge functionality thông qua LayerZero
 */
contract DiamondTokenETH is ERC20, ERC20Burnable, AccessControl, Pausable, OFTV2 {
    bytes32 public constant MINTER_ROLE = keccak256("MINTER_ROLE");
    bytes32 public constant BRIDGE_ROLE = keccak256("BRIDGE_ROLE");
    bytes32 public constant PAUSER_ROLE = keccak256("PAUSER_ROLE");
    bytes32 public constant TIER_MANAGER_ROLE = keccak256("TIER_MANAGER_ROLE");
    
    uint8 private constant _decimals = 18;
    uint256 private constant _maxSupply = 1_000_000_000 * 10**18; // 1 tỷ token
    
    // Cấu trúc dữ liệu lưu trữ thông tin tier của người dùng
    mapping(address => uint8) private _userTiers;
    
    // Danh sách token được phép sử dụng theo tier
    mapping(address => uint8) private _tokenTiers;
    
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
    ) public payable onlyRole(BRIDGE_ROLE) {
        _send(dstChainId, to, amount, refundAddress, zroPaymentAddress, adapterParams);
    }
    
    /**
     * @dev Ước tính phí bridge
     * @param dstChainId Chain ID đích theo LayerZero
     * @param adapterParams Tham số cho adapter
     * @return Phí nativeAmount (gas fee) và zroFee
     */
    function estimateSendFee(
        uint16 dstChainId,
        bytes memory toAddress,
        uint256 amount,
        bytes memory adapterParams
    ) public view returns (uint256 nativeFee, uint256 zroFee) {
        return _estimateSendFee(dstChainId, toAddress, amount, false, adapterParams);
    }
} 