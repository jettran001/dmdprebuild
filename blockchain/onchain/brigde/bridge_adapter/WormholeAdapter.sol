// SPDX-License-Identifier: MIT
pragma solidity 0.8.20;

import "./IBridgeAdapter.sol";

/**
 * @title IWormhole
 * @dev Interface cho Wormhole protocol
 */
interface IWormhole {
    function publishMessage(
        uint32 nonce,
        bytes memory payload,
        uint8 consistencyLevel
    ) external payable returns (uint64 sequence);
}

/**
 * @title WormholeAdapter
 * @dev Adapter kết nối với Wormhole protocol cho bridging
 */
contract WormholeAdapter is IBridgeAdapter {
    // Constants
    string private constant ADAPTER_TYPE = "Wormhole";
    uint16 private immutable _targetChain;
    uint16[] private _supportedChains;
    
    // Wormhole contract
    address public wormhole;
    uint8 public consistencyLevel = 1; // finality level
    
    // Events
    event MessageSent(uint64 sequence, bytes payload);
    event WormholeAddressUpdated(address indexed newAddress);
    
    /**
     * @dev Khởi tạo WormholeAdapter
     * @param _wormhole Địa chỉ contract Wormhole
     * @param targetChainId ID của chain đích
     * @param supportedChainIds Danh sách các chain được hỗ trợ
     */
    constructor(
        address _wormhole,
        uint16 targetChainId,
        uint16[] memory supportedChainIds
    ) {
        require(_wormhole != address(0), "Invalid Wormhole address");
        wormhole = _wormhole;
        _targetChain = targetChainId;
        _supportedChains = supportedChainIds;
    }
    
    /**
     * @dev Bridge token đến chain đích qua Wormhole
     * @param dstChainId ID của chain đích
     * @param destination Địa chỉ đích được mã hóa
     * @param amount Số lượng token cần bridge
     */
    function bridgeTo(
        uint16 dstChainId,
        bytes calldata destination,
        uint256 amount
    ) external payable override {
        require(isChainSupported(dstChainId), "Chain not supported");
        
        // Đóng gói payload với thông tin cần thiết
        bytes memory payload = abi.encode(destination, amount, msg.sender);
        
        // Gửi qua Wormhole
        uint64 sequence = IWormhole(wormhole).publishMessage{value: msg.value}(
            uint32(block.timestamp), // nonce
            payload,
            consistencyLevel
        );
        
        emit BridgeInitiated(msg.sender, dstChainId, destination, amount);
        emit MessageSent(sequence, payload);
    }
    
    /**
     * @dev Ước tính phí bridge (phí cố định cho Wormhole)
     * @param dstChainId ID của chain đích
     * @param amount Số lượng token cần bridge (không sử dụng trong tính phí)
     * @return Phí bridge ước tính
     */
    function estimateFee(uint16 dstChainId, uint256 amount) external view override returns (uint256) {
        require(isChainSupported(dstChainId), "Chain not supported");
        
        // Wormhole có phí cố định cho mỗi message
        // Đây là giá trị mẫu, cần được cập nhật tùy thuộc vào cấu hình thực tế
        return 0.01 ether;
    }
    
    /**
     * @dev Trả về danh sách các chain được hỗ trợ
     * @return Danh sách ID của các chain được hỗ trợ
     */
    function supportedChains() external view override returns (uint16[] memory) {
        return _supportedChains;
    }
    
    /**
     * @dev Trả về loại adapter
     * @return Tên loại adapter (Wormhole)
     */
    function adapterType() external pure override returns (string memory) {
        return ADAPTER_TYPE;
    }
    
    /**
     * @dev Trả về ID của chain đích mà adapter này hỗ trợ
     * @return ID của chain đích
     */
    function targetChain() external view override returns (uint16) {
        return _targetChain;
    }
    
    /**
     * @dev Kiểm tra xem chain có được hỗ trợ không
     * @param chainId ID của chain cần kiểm tra
     * @return true nếu chain được hỗ trợ
     */
    function isChainSupported(uint16 chainId) public view override returns (bool) {
        for (uint256 i = 0; i < _supportedChains.length; i++) {
            if (_supportedChains[i] == chainId) {
                return true;
            }
        }
        return false;
    }
    
    /**
     * @dev Cập nhật địa chỉ contract Wormhole
     * @param _wormhole Địa chỉ mới của contract Wormhole
     */
    function updateWormholeAddress(address _wormhole) external {
        // Thêm modifier onlyOwner hoặc quyền admin tại đây
        require(_wormhole != address(0), "Invalid Wormhole address");
        wormhole = _wormhole;
        emit WormholeAddressUpdated(_wormhole);
    }
    
    /**
     * @dev Cập nhật mức độ consistency của Wormhole
     * @param _consistencyLevel Mức độ consistency mới
     */
    function updateConsistencyLevel(uint8 _consistencyLevel) external {
        // Thêm modifier onlyOwner hoặc quyền admin tại đây
        consistencyLevel = _consistencyLevel;
    }
}
