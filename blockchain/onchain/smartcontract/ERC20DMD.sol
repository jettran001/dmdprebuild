// SPDX-License-Identifier: MIT
pragma solidity 0.8.17;

import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/security/Pausable.sol";

// Interface cho DiamondToken ERC1155
interface IDiamondToken {
    function unwrapFromERC20(address to, uint256 amount) external;
    function bridgedSupply() external view returns (uint256);
    function DMD_TOKEN_ID() external view returns (uint256);
}

// Interface cho BridgeERC20Proxy
interface IBridgeERC20Proxy {
    function bridgeToNear(address from, bytes calldata nearAddress, uint256 amount) external payable;
    function estimateBridgeFee(uint16 chainId) external view returns (uint256);
}

/**
 * @title ERC20DMD
 * @dev ERC20 wrapper token for Diamond Token (DMD)
 * Acts as an intermediate representation for bridging between ERC1155 and other chains
 */
contract ERC20DMD is ERC20, Ownable, Pausable {
    // Địa chỉ của DiamondToken ERC1155
    IDiamondToken public dmdToken;
    
    // Địa chỉ của bridge proxy
    IBridgeERC20Proxy public bridgeProxy;
    
    // LayerZero chain ID for NEAR
    uint16 public constant NEAR_CHAIN_ID = 115;
    
    // Events
    event BridgeToNear(address indexed from, bytes nearAddress, uint256 amount);
    event BridgeProxySet(address indexed proxy);
    event WrappedFromERC1155(address indexed user, uint256 amount);
    event UnwrappedToERC1155(address indexed user, uint256 amount);
    
    /**
     * @dev Khởi tạo contract
     * @param name Tên token
     * @param symbol Symbol token
     * @param _dmdToken Địa chỉ DiamondToken ERC1155
     */
    constructor(string memory name, string memory symbol, address _dmdToken) ERC20(name, symbol) {
        require(_dmdToken != address(0), "DMD token address cannot be zero");
        dmdToken = IDiamondToken(_dmdToken);
    }
    
    /**
     * @dev Set bridge proxy address
     * @param _proxy Địa chỉ bridge proxy
     */
    function setBridgeProxy(address _proxy) external onlyOwner {
        require(_proxy != address(0), "Proxy address cannot be zero");
        bridgeProxy = IBridgeERC20Proxy(_proxy);
        emit BridgeProxySet(_proxy);
    }
    
    /**
     * @dev Mint wrapped tokens (chỉ được gọi từ DiamondToken)
     * @param to Địa chỉ nhận token
     * @param amount Số lượng token
     */
    function mintWrapped(address to, uint256 amount) external {
        // Chỉ DiamondToken hoặc Bridge Proxy mới được phép gọi
        require(
            msg.sender == address(dmdToken) || 
            msg.sender == address(bridgeProxy), 
            "Unauthorized: only DMD token or bridge proxy"
        );
        
        _mint(to, amount);
        emit WrappedFromERC1155(to, amount);
    }
    
    /**
     * @dev Burn wrapped tokens
     * @param from Địa chỉ bị burn
     * @param amount Số lượng token
     */
    function burnWrapped(address from, uint256 amount) external {
        require(
            msg.sender == address(dmdToken) || 
            msg.sender == address(bridgeProxy), 
            "Unauthorized: only DMD token or bridge proxy"
        );
        
        _burn(from, amount);
    }
    
    /**
     * @dev Unwrap ERC20 thành ERC1155
     * @param amount Số lượng token
     */
    function unwrapToERC1155(uint256 amount) external whenNotPaused {
        require(balanceOf(msg.sender) >= amount, "Insufficient balance");
        
        // Burn ERC20
        _burn(msg.sender, amount);
        
        // Mint ERC1155
        dmdToken.unwrapFromERC20(msg.sender, amount);
        
        emit UnwrappedToERC1155(msg.sender, amount);
    }
    
    /**
     * @dev Bridge token sang NEAR
     * @param nearAddress Địa chỉ NEAR dạng bytes
     * @param amount Số lượng token
     */
    function bridgeToNear(bytes calldata nearAddress, uint256 amount) external payable whenNotPaused {
        require(address(bridgeProxy) != address(0), "Bridge proxy not set");
        require(balanceOf(msg.sender) >= amount, "Insufficient balance");
        
        // Chuyển token vào bridge proxy
        _transfer(msg.sender, address(bridgeProxy), amount);
        
        // Gọi bridge proxy để xử lý
        bridgeProxy.bridgeToNear{value: msg.value}(msg.sender, nearAddress, amount);
        
        emit BridgeToNear(msg.sender, nearAddress, amount);
    }
    
    /**
     * @dev Tạm dừng các hoạt động
     */
    function pause() external onlyOwner {
        _pause();
    }
    
    /**
     * @dev Mở lại các hoạt động
     */
    function unpause() external onlyOwner {
        _unpause();
    }
    
    /**
     * @dev Hook kiểm tra trước khi chuyển token
     */
    function _beforeTokenTransfer(
        address from,
        address to,
        uint256 amount
    ) internal override whenNotPaused {
        super._beforeTokenTransfer(from, to, amount);
    }
    
    /**
     * @dev Lấy phí bridge ước tính
     */
    function estimateBridgeFee() external view returns (uint256) {
        require(address(bridgeProxy) != address(0), "Bridge proxy not set");
        return bridgeProxy.estimateBridgeFee(NEAR_CHAIN_ID);
    }
} 