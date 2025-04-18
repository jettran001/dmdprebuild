// SPDX-License-Identifier: MIT
pragma solidity 0.8.18;

/**
 * @title ITokenHub Interface
 * @dev Interface cho TokenHub - Quản lý token trên nhiều blockchain
 * @dev Chain ID: 0=ETHEREUM, 1=BSC, 2=POLYGON, 3=ARBITRUM, 4=AVALANCHE, 5=SOLANA, 6=NEAR, 7=OPTIMISM, 8=FANTOM
 * @dev Bridge Status: 0=PENDING, 1=PROCESSING, 2=COMPLETED, 3=FAILED
 */
interface ITokenHub {
    /**
     * @dev Event khi bridge được khởi tạo
     */
    event BridgeInitiated(
        bytes32 indexed bridgeId,
        address indexed sender,
        uint8 fromChain,
        uint8 toChain,
        address recipient,
        uint256 amount,
        uint256 fee
    );
    
    /**
     * @dev Event khi bridge hoàn thành
     */
    event BridgeCompleted(
        bytes32 indexed bridgeId,
        uint8 fromChain, 
        uint8 toChain
    );
    
    /**
     * @dev Event khi bridge thất bại
     */
    event BridgeFailed(
        bytes32 indexed bridgeId,
        uint8 fromChain,
        uint8 toChain,
        string reason
    );
    
    /**
     * @dev Khởi tạo bridge từ chain hiện tại sang chain khác
     * @param toChain Chain đích (0-8)
     * @param recipient Địa chỉ nhận ở chain đích
     * @param amount Số lượng token
     * @return bridgeId ID của giao dịch bridge
     */
    function bridge(uint8 toChain, address recipient, uint256 amount) external payable returns (bytes32 bridgeId);
    
    /**
     * @dev Lấy thông tin bridge
     * @param bridgeId ID của giao dịch bridge
     * @return sender Địa chỉ người gửi
     * @return fromChain Chain gốc
     * @return toChain Chain đích  
     * @return recipient Địa chỉ nhận
     * @return amount Số lượng token
     * @return fee Phí giao dịch
     * @return timestamp Thời gian khởi tạo
     * @return status Trạng thái bridge (0-3)
     */
    function getBridgeInfo(bytes32 bridgeId) external view returns (
        address sender,
        uint8 fromChain,
        uint8 toChain,
        address recipient,
        uint256 amount,
        uint256 fee,
        uint256 timestamp,
        uint8 status
    );
    
    /**
     * @dev Lấy thông tin tổng cung token trên một chain
     * @param chain Chain cần kiểm tra (0-8)
     * @return Tổng cung token
     */
    function getTotalSupply(uint8 chain) external view returns (uint256);
    
    /**
     * @dev Lấy số dư token của một địa chỉ trên một chain
     * @param account Địa chỉ cần kiểm tra
     * @param chain Chain cần kiểm tra (0-8)
     * @return Số dư token
     */
    function getBalance(address account, uint8 chain) external view returns (uint256);
    
    /**
     * @dev Ước tính phí bridge
     * @param toChain Chain đích (0-8)
     * @param amount Số lượng token
     * @return baseFee Phí cơ bản (token fee)
     * @return gasFee Phí gas
     */
    function estimateBridgeFee(uint8 toChain, uint256 amount) external view returns (uint256 baseFee, uint256 gasFee);
    
    /**
     * @dev Đồng bộ dữ liệu giữa các chain
     */
    function syncData() external;
} 