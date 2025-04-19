// SPDX-License-Identifier: MIT
pragma solidity 0.8.20;

import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/security/Pausable.sol";
import "@openzeppelin/contracts/security/ReentrancyGuard.sol";

// Interface cho DiamondToken ERC1155
interface IDiamondToken {
    function unwrapFromERC20(address to, uint256 amount) external;
    function balanceOf(address account, uint256 id) external view returns (uint256);
}

/**
 * @title Wrapped DMD Token (wDMD)
 * @dev ERC20 wrapper token cho DiamondToken (DMD) ERC1155
 * Được sử dụng làm token trung gian để bridge giữa ERC1155 và NEAR
 */
contract WrappedDMD is ERC20, Ownable, Pausable, ReentrancyGuard {
    // Địa chỉ của DiamondToken ERC1155
    IDiamondToken public dmdToken;
    
    // Địa chỉ của bridge proxy
    address public bridgeProxy;
    
    // ID của DMD token trong ERC1155
    uint256 public constant DMD_TOKEN_ID = 0;
    
    // Events
    event BridgeProxySet(address indexed proxy);
    event WrappedFromERC1155(address indexed user, uint256 amount);
    event UnwrappedToERC1155(address indexed user, uint256 amount);
    
    /**
     * @dev Khởi tạo contract Wrapped DMD
     * @param _dmdToken Địa chỉ DiamondToken ERC1155
     */
    constructor(address _dmdToken) ERC20("Wrapped DMD", "wDMD") {
        require(_dmdToken != address(0), "DMD token address cannot be zero");
        dmdToken = IDiamondToken(_dmdToken);
    }
    
    /**
     * @dev Set bridge proxy address
     * @param _proxy Địa chỉ bridge proxy
     */
    function setBridgeProxy(address _proxy) external onlyOwner {
        require(_proxy != address(0), "Proxy address cannot be zero");
        bridgeProxy = _proxy;
        emit BridgeProxySet(_proxy);
    }
    
    /**
     * @dev Mint wrapped tokens (chỉ được gọi từ DiamondToken hoặc Bridge Proxy)
     * @param to Địa chỉ nhận token
     * @param amount Số lượng token
     */
    function mintWrapped(address to, uint256 amount) external {
        require(
            msg.sender == address(dmdToken) || 
            msg.sender == bridgeProxy, 
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
            msg.sender == bridgeProxy, 
            "Unauthorized: only DMD token or bridge proxy"
        );
        
        // Nếu người gọi là bridge proxy, kiểm tra xem 'from' có đủ token không
        if (msg.sender == bridgeProxy) {
            require(balanceOf(from) >= amount, "Insufficient balance");
            _burn(from, amount);
        } else {
            _burn(from, amount);
        }
    }
    
    /**
     * @dev Wrap ERC1155 thành ERC20
     * @param amount Số lượng token
     */
    function wrapFromERC1155(uint256 amount) external whenNotPaused {
        // Kiểm tra số dư ERC1155
        uint256 erc1155Balance = dmdToken.balanceOf(msg.sender, DMD_TOKEN_ID);
        require(erc1155Balance >= amount, "Insufficient ERC1155 balance");
        
        // Chuyển đổi ERC1155 -> ERC20
        dmdToken.unwrapFromERC20(address(this), amount);
        
        // Mint ERC20 wrapper token
        _mint(msg.sender, amount);
        
        emit WrappedFromERC1155(msg.sender, amount);
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
     * @dev Chuyển token sang bridge proxy để bridge sang NEAR
     * @param nearAddress Địa chỉ người nhận trên NEAR
     * @param amount Số lượng token
     */
    function bridgeToNear(bytes calldata nearAddress, uint256 amount) external payable whenNotPaused nonReentrant {
        require(bridgeProxy != address(0), "Bridge proxy not set");
        require(balanceOf(msg.sender) >= amount, "Insufficient balance");
        require(nearAddress.length > 0, "Empty destination address");
        
        // Check - Validate inputs and state
        address sender = msg.sender;
        uint256 msgValue = msg.value;
        
        // Effects - Update state
        // Burn tokens before external call to prevent reentrancy
        _burn(sender, amount);
        
        // Interactions - Call external contract
        (bool success, ) = bridgeProxy.call{value: msgValue}(
            abi.encodeWithSignature(
                "bridgeToNear(address,bytes,uint256)",
                sender,
                nearAddress,
                amount
            )
        );
        
        // Revert if bridge call failed
        require(success, "Bridge call failed");
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
     * @dev Get total DMD (ERC1155 + ERC20) balance for an account
     * @param account Địa chỉ cần kiểm tra
     * @return Tổng số dư (ERC1155 + ERC20)
     */
    function getTotalBalance(address account) external view returns (uint256) {
        return dmdToken.balanceOf(account, DMD_TOKEN_ID) + balanceOf(account);
    }
}
