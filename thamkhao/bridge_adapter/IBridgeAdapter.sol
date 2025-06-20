// SPDX-License-Identifier: MIT
pragma solidity 0.8.20;

/**
 * @title IBridgeAdapter
 * @dev Interface chung cho các bridge adapter, đảm bảo chuẩn hóa API
 */
interface IBridgeAdapter {
    /**
     * @dev Gửi token qua bridge đến chain đích
     * @param dstChainId ID của chain đích
     * @param destination Địa chỉ đích được mã hóa
     * @param amount Số lượng token cần bridge
     */
    function bridgeTo(uint16 dstChainId, bytes calldata destination, uint256 amount) external payable;
    
    /**
     * @dev Ước tính phí để bridge đến chain đích
     * @param dstChainId ID của chain đích
     * @param amount Số lượng token cần bridge
     * @return Phí bridge ước tính
     */
    function estimateFee(uint16 dstChainId, uint256 amount) external view returns (uint256);
    
    /**
     * @dev Trả về danh sách các chain được hỗ trợ
     * @return Danh sách ID của các chain được hỗ trợ
     */
    function supportedChains() external view returns (uint16[] memory);
    
    /**
     * @dev Trả về loại adapter (VD: "LayerZero", "Wormhole")
     * @return Tên loại adapter
     */
    function adapterType() external view returns (string memory);
    
    /**
     * @dev Trả về ID của chain đích mà adapter này hỗ trợ
     * @return ID của chain đích
     */
    function targetChain() external view returns (uint16);
    
    /**
     * @dev Kiểm tra xem chain có được hỗ trợ không
     * @param chainId ID của chain cần kiểm tra
     * @return true nếu chain được hỗ trợ
     */
    function isChainSupported(uint16 chainId) external view returns (bool);
    
    /**
     * @dev Lấy tên của chain theo ID
     * @param chainId ID của chain cần lấy tên
     * @return Tên của chain
     */
    function getChainName(uint16 chainId) external view returns (string memory);
    
    /**
     * @dev Sự kiện khi bridge thành công
     */
    event BridgeInitiated(address indexed sender, uint16 dstChainId, bytes destination, uint256 amount);
    
    /**
     * @dev Sự kiện khi nhận token từ bridge
     */
    event BridgeCompleted(uint16 srcChainId, address indexed receiver, uint256 amount);
    
    /**
     * @dev Sự kiện khi bridge thất bại
     */
    event BridgeFailed(address indexed sender, uint16 dstChainId, bytes destination, uint256 amount, string reason);
}