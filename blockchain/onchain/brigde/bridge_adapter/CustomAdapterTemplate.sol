// SPDX-License-Identifier: MIT
pragma solidity 0.8.20;

import "./IBridgeAdapter.sol";

/**
 * @title CustomAdapterTemplate
 * @dev Template để triển khai bridge adapter tùy chỉnh tuân theo IBridgeAdapter
 * Sao chép file này và điền thông tin cần thiết để tạo adapter mới
 */
contract CustomAdapterTemplate is IBridgeAdapter {
    // Các biến lưu trữ
    uint16 private targetChainId;       // Chain ID mục tiêu
    uint16[] private supportedChainIds; // Danh sách các chain được hỗ trợ
    string private adapterTypeName;     // Tên loại adapter (ví dụ: "Axelar", "Multichain")
    address private owner;              // Địa chỉ chủ sở hữu adapter
    address private messageEndpoint;    // Địa chỉ của message endpoint (nếu cần)
    
    // Sự kiện
    event MessageSent(uint16 indexed dstChainId, bytes destination, uint256 amount);
    event OwnershipTransferred(address indexed previousOwner, address indexed newOwner);
    
    // Modifier
    modifier onlyOwner() {
        require(msg.sender == owner, "CustomAdapter: caller is not the owner");
        _;
    }
    
    /**
     * @dev Khởi tạo adapter với thông tin cần thiết
     * @param _messageEndpoint Địa chỉ của message endpoint (ví dụ: Axelar Gateway, LayerZero Endpoint)
     * @param _targetChainId Chain ID mục tiêu chính của adapter
     * @param _supportedChainIds Danh sách các chain được hỗ trợ
     */
    constructor(
        address _messageEndpoint,
        uint16 _targetChainId,
        uint16[] memory _supportedChainIds
    ) {
        require(_messageEndpoint != address(0), "CustomAdapter: invalid endpoint address");
        
        owner = msg.sender;
        messageEndpoint = _messageEndpoint;
        targetChainId = _targetChainId;
        
        // Thiết lập các chain được hỗ trợ
        for (uint i = 0; i < _supportedChainIds.length; i++) {
            supportedChainIds.push(_supportedChainIds[i]);
        }
        
        // Thay đổi tên loại adapter của bạn tại đây
        adapterTypeName = "CustomAdapter";
    }
    
    /**
     * @dev Gửi token qua bridge đến chain đích
     * @param dstChainId ID của chain đích
     * @param destination Địa chỉ đích được mã hóa
     * @param amount Số lượng token cần bridge
     */
    function bridgeTo(uint16 dstChainId, bytes calldata destination, uint256 amount) external payable override {
        require(isChainSupported(dstChainId), "CustomAdapter: chain not supported");
        
        // TODO: Triển khai logic bridge cụ thể ở đây
        // Ví dụ:
        // 1. Gọi hàm sendMessage của MessageEndpoint (Axelar, LayerZero, v.v.)
        // 2. Xử lý phí và token
        
        // Mẫu gọi endpoint, cần tùy chỉnh dựa trên protocol cụ thể:
        // IMessageEndpoint(messageEndpoint).sendMessage{value: msg.value}(
        //     dstChainId,
        //     destination,
        //     abi.encode(amount)
        // );
        
        emit MessageSent(dstChainId, destination, amount);
        emit BridgeInitiated(msg.sender, dstChainId, destination, amount);
    }
    
    /**
     * @dev Ước tính phí để bridge đến chain đích
     * @param dstChainId ID của chain đích
     * @param amount Số lượng token cần bridge
     * @return Phí bridge ước tính
     */
    function estimateFee(uint16 dstChainId, uint256 amount) external view override returns (uint256) {
        require(isChainSupported(dstChainId), "CustomAdapter: chain not supported");
        
        // TODO: Triển khai logic ước tính phí ở đây
        // Ví dụ:
        // return IMessageEndpoint(messageEndpoint).estimateFee(dstChainId, abi.encode(amount));
        
        // Trả về phí mẫu, cần thay thế bằng ước tính thực tế
        return 0.001 ether;
    }
    
    /**
     * @dev Trả về danh sách các chain được hỗ trợ
     * @return Danh sách ID của các chain được hỗ trợ
     */
    function supportedChains() external view override returns (uint16[] memory) {
        return supportedChainIds;
    }
    
    /**
     * @dev Trả về loại adapter
     * @return Tên loại adapter
     */
    function adapterType() external view override returns (string memory) {
        return adapterTypeName;
    }
    
    /**
     * @dev Trả về ID của chain đích mà adapter này hỗ trợ
     * @return ID của chain đích
     */
    function targetChain() external view override returns (uint16) {
        return targetChainId;
    }
    
    /**
     * @dev Kiểm tra xem chain có được hỗ trợ không
     * @param chainId ID của chain cần kiểm tra
     * @return true nếu chain được hỗ trợ
     */
    function isChainSupported(uint16 chainId) public view override returns (bool) {
        for (uint i = 0; i < supportedChainIds.length; i++) {
            if (supportedChainIds[i] == chainId) {
                return true;
            }
        }
        return false;
    }
    
    /**
     * @dev Thêm chain mới vào danh sách hỗ trợ
     * @param chainId ID của chain cần thêm
     */
    function addSupportedChain(uint16 chainId) external onlyOwner {
        require(!isChainSupported(chainId), "CustomAdapter: chain already supported");
        supportedChainIds.push(chainId);
    }
    
    /**
     * @dev Loại bỏ chain khỏi danh sách hỗ trợ
     * @param chainId ID của chain cần loại bỏ
     */
    function removeSupportedChain(uint16 chainId) external onlyOwner {
        require(isChainSupported(chainId), "CustomAdapter: chain not supported");
        
        for (uint i = 0; i < supportedChainIds.length; i++) {
            if (supportedChainIds[i] == chainId) {
                // Thay thế bằng phần tử cuối cùng và xóa phần tử cuối
                supportedChainIds[i] = supportedChainIds[supportedChainIds.length - 1];
                supportedChainIds.pop();
                break;
            }
        }
    }
    
    /**
     * @dev Cập nhật địa chỉ message endpoint
     * @param newEndpoint Địa chỉ endpoint mới
     */
    function updateMessageEndpoint(address newEndpoint) external onlyOwner {
        require(newEndpoint != address(0), "CustomAdapter: invalid endpoint address");
        messageEndpoint = newEndpoint;
    }
    
    /**
     * @dev Cập nhật tên loại adapter
     * @param newTypeName Tên loại mới
     */
    function updateAdapterType(string memory newTypeName) external onlyOwner {
        require(bytes(newTypeName).length > 0, "CustomAdapter: empty type name");
        adapterTypeName = newTypeName;
    }
    
    /**
     * @dev Chuyển quyền sở hữu adapter
     * @param newOwner Địa chỉ chủ sở hữu mới
     */
    function transferOwnership(address newOwner) external onlyOwner {
        require(newOwner != address(0), "CustomAdapter: invalid owner address");
        emit OwnershipTransferred(owner, newOwner);
        owner = newOwner;
    }
    
    /**
     * @dev Rút ETH từ contract (cho trường hợp khẩn cấp)
     * @param amount Số lượng ETH cần rút
     */
    function withdrawETH(uint256 amount) external onlyOwner {
        require(amount <= address(this).balance, "CustomAdapter: insufficient balance");
        payable(owner).transfer(amount);
    }
} 