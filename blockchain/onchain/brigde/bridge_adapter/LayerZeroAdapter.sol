// SPDX-License-Identifier: MIT
pragma solidity 0.8.20;

import "./IBridgeAdapter.sol";
import "@layerzerolabs/solidity-examples/contracts/lzApp/NonblockingLzApp.sol";

/**
 * @title LayerZeroAdapter
 * @dev Adapter kết nối với LayerZero protocol cho bridging
 */
contract LayerZeroAdapter is IBridgeAdapter, NonblockingLzApp {
    // Constants
    string private constant ADAPTER_TYPE = "LayerZero";
    uint16 private immutable _targetChain;
    uint16[] private _supportedChains;
    
    // Events
    event MessageSent(uint16 indexed chainId, bytes payload, bytes receiver);
    event MessageReceived(uint16 indexed srcChainId, bytes payload);
    
    /**
     * @dev Khởi tạo LayerZeroAdapter
     * @param _lzEndpoint Địa chỉ endpoint của LayerZero
     * @param targetChainId ID của chain đích
     * @param supportedChainIds Danh sách các chain được hỗ trợ
     */
    constructor(
        address _lzEndpoint, 
        uint16 targetChainId,
        uint16[] memory supportedChainIds
    ) NonblockingLzApp(_lzEndpoint) {
        _targetChain = targetChainId;
        _supportedChains = supportedChainIds;
    }
    
    /**
     * @dev Bridge token đến chain đích qua LayerZero
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
        
        // Gửi qua LayerZero
        _lzSend(
            dstChainId,
            payload,
            payable(msg.sender),
            address(0x0),
            bytes(""),
            msg.value
        );
        
        emit BridgeInitiated(msg.sender, dstChainId, destination, amount);
        emit MessageSent(dstChainId, payload, destination);
    }
    
    /**
     * @dev Nhận message từ LayerZero và xử lý
     * @param _srcChainId ID của chain nguồn
     * @param _srcAddress Địa chỉ nguồn đã mã hóa
     * @param _nonce Nonce của giao dịch
     * @param _payload Payload chứa thông tin giao dịch
     */
    function _nonblockingLzReceive(
        uint16 _srcChainId,
        bytes memory _srcAddress,
        uint64 _nonce,
        bytes memory _payload
    ) internal override {
        // Giải mã payload
        (bytes memory receiverAddress, uint256 amount, address sender) = abi.decode(
            _payload,
            (bytes, uint256, address)
        );
        
        // Xử lý bridge (để implement tùy thuộc vào logic cụ thể)
        // Ví dụ: mint token cho người nhận
        
        emit BridgeCompleted(_srcChainId, address(bytes20(receiverAddress)), amount);
        emit MessageReceived(_srcChainId, _payload);
    }
    
    /**
     * @dev Ước tính phí bridge
     * @param dstChainId ID của chain đích
     * @param amount Số lượng token cần bridge (không sử dụng trong tính phí)
     * @return Phí bridge ước tính
     */
    function estimateFee(uint16 dstChainId, uint256 amount) external view override returns (uint256) {
        require(isChainSupported(dstChainId), "Chain not supported");
        
        bytes memory payload = abi.encode(bytes(""), amount, address(0));
        (uint256 fee, ) = lzEndpoint.estimateFees(
            dstChainId,
            address(this),
            payload,
            false,
            bytes("")
        );
        
        return fee;
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
     * @return Tên loại adapter (LayerZero)
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
}
