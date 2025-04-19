// SPDX-License-Identifier: MIT
pragma solidity 0.8.20;

import "@openzeppelin/contracts/token/ERC1155/IERC1155.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/security/Pausable.sol";
import "@openzeppelin/contracts/token/ERC1155/utils/ERC1155Holder.sol";

// Interface cho WrappedDMD token (ERC20)
interface IWrappedDMD {
    function mintWrapped(address to, uint256 amount) external;
    function burnWrapped(address from, uint256 amount) external;
}

// Interface cho DiamondToken (ERC1155)
interface IDiamondTokenERC1155 {
    function safeTransferFrom(address from, address to, uint256 id, uint256 amount, bytes calldata data) external;
    function balanceOf(address account, uint256 id) external view returns (uint256);
}

/**
 * @title ERC1155 Wrapper
 * @dev Bộ đóng gói ERC1155 thành ERC20 (WrappedDMD)
 * Nhận ERC1155 từ user, lock nó trong contract, và mint ra ERC20 wrapped
 */
contract ERC1155Wrapper is Ownable, Pausable, ERC1155Holder {
    // Địa chỉ của DiamondToken ERC1155
    IDiamondTokenERC1155 public dmdToken;
    
    // Địa chỉ của WrappedDMD ERC20
    IWrappedDMD public wrappedDmd;
    
    // ID của DMD token trong ERC1155
    uint256 public constant DMD_TOKEN_ID = 0;
    
    // Events
    event Wrapped(address indexed user, uint256 amount);
    event Unwrapped(address indexed user, uint256 amount);
    
    /**
     * @dev Khởi tạo contract
     * @param _dmdToken Địa chỉ DiamondToken ERC1155
     * @param _wrappedDmd Địa chỉ WrappedDMD ERC20
     */
    constructor(address _dmdToken, address _wrappedDmd) {
        require(_dmdToken != address(0), "DMD token address cannot be zero");
        require(_wrappedDmd != address(0), "Wrapped DMD address cannot be zero");
        dmdToken = IDiamondTokenERC1155(_dmdToken);
        wrappedDmd = IWrappedDMD(_wrappedDmd);
    }
    
    /**
     * @dev Wrap ERC1155 thành ERC20
     * @param amount Số lượng token cần wrap
     */
    function wrap(uint256 amount) external whenNotPaused {
        // Kiểm tra số dư ERC1155
        uint256 erc1155Balance = dmdToken.balanceOf(msg.sender, DMD_TOKEN_ID);
        require(erc1155Balance >= amount, "Insufficient ERC1155 balance");
        
        // Chuyển ERC1155 từ user vào contract này
        dmdToken.safeTransferFrom(msg.sender, address(this), DMD_TOKEN_ID, amount, "");
        
        // Mint ERC20 wrapped token
        wrappedDmd.mintWrapped(msg.sender, amount);
        
        emit Wrapped(msg.sender, amount);
    }
    
    /**
     * @dev Unwrap ERC20 thành ERC1155 (chỉ được gọi từ WrappedDMD)
     * @param to Địa chỉ nhận token ERC1155
     * @param amount Số lượng token cần unwrap
     */
    function unwrap(address to, uint256 amount) external whenNotPaused {
        require(msg.sender == address(wrappedDmd), "Only wrapped DMD contract can call");
        
        // Chuyển ERC1155 từ contract này đến user
        dmdToken.safeTransferFrom(address(this), to, DMD_TOKEN_ID, amount, "");
        
        emit Unwrapped(to, amount);
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
}
