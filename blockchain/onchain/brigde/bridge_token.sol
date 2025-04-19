// SPDX-License-Identifier: MIT
pragma solidity 0.8.20;

// Chú ý: Nhập các thư viện OpenZeppelin sau khi đã cài đặt
import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import "@openzeppelin/contracts/token/ERC20/extensions/ERC20Burnable.sol";
import "@openzeppelin/contracts/access/AccessControl.sol";
import "@openzeppelin/contracts/security/Pausable.sol";

/**
 * @title BridgeToken
 * @dev ERC20 token được sử dụng cho bridge giữa các mạng.
 * Contract này là interface chuẩn cho bridge token trên các mạng EVM.
 */
contract BridgeToken is ERC20, ERC20Burnable, AccessControl, Pausable {
    bytes32 public constant MINTER_ROLE = keccak256("MINTER_ROLE");
    bytes32 public constant BRIDGE_ROLE = keccak256("BRIDGE_ROLE");
    bytes32 public constant PAUSER_ROLE = keccak256("PAUSER_ROLE");
    
    uint8 private _decimals;

    event BridgeMinted(address indexed to, uint256 amount, string srcChain);
    event BridgeBurned(address indexed from, uint256 amount, string dstChain);
    
    constructor(
        string memory name,
        string memory symbol,
        uint8 tokenDecimals,
        address admin
    ) ERC20(name, symbol) {
        _decimals = tokenDecimals;
        
        _setupRole(DEFAULT_ADMIN_ROLE, admin);
        _setupRole(MINTER_ROLE, admin);
        _setupRole(BRIDGE_ROLE, admin);
        _setupRole(PAUSER_ROLE, admin);
    }
    
    /**
     * @dev Override decimals() để sử dụng giá trị decimals tùy chỉnh
     */
    function decimals() public view virtual override returns (uint8) {
        return _decimals;
    }
    
    /**
     * @dev Mint token thông qua bridge
     * @param to Địa chỉ nhận token
     * @param amount Số lượng token
     * @param srcChain Chuỗi nguồn
     */
    function bridgeMint(address to, uint256 amount, string calldata srcChain) 
        external 
        onlyRole(BRIDGE_ROLE) 
        whenNotPaused 
    {
        _mint(to, amount);
        emit BridgeMinted(to, amount, srcChain);
    }
    
    /**
     * @dev Burn token để bridge sang chain khác
     * @param from Địa chỉ chủ token
     * @param amount Số lượng token
     * @param dstChain Chuỗi đích
     */
    function bridgeBurn(address from, uint256 amount, string calldata dstChain) 
        external 
        onlyRole(BRIDGE_ROLE) 
        whenNotPaused 
    {
        _burn(from, amount);
        emit BridgeBurned(from, amount, dstChain);
    }
    
    /**
     * @dev Mint token (chỉ cho admin và minter)
     * @param to Địa chỉ nhận token
     * @param amount Số lượng token
     */
    function mint(address to, uint256 amount) 
        external 
        onlyRole(MINTER_ROLE) 
        whenNotPaused 
    {
        _mint(to, amount);
    }
    
    /**
     * @dev Pause contract
     */
    function pause() external onlyRole(PAUSER_ROLE) {
        _pause();
    }
    
    /**
     * @dev Unpause contract
     */
    function unpause() external onlyRole(PAUSER_ROLE) {
        _unpause();
    }
    
    /**
     * @dev Hook ghi đè để kiểm tra trạng thái pause khi chuyển token
     */
    function _beforeTokenTransfer(
        address from,
        address to,
        uint256 amount
    ) internal virtual override whenNotPaused {
        super._beforeTokenTransfer(from, to, amount);
    }
} 