// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

/**
 * @dev Interface cho LayerZero Receiver.
 * Các contract muốn nhận message từ LayerZero phải implement interface này
 */
interface ILayerZeroReceiver {
    /**
     * @dev Nhận message từ LayerZero Endpoint
     * @param _srcChainId Chain ID nguồn
     * @param _srcAddress Địa chỉ nguồn trên chain nguồn (dưới dạng bytes)
     * @param _nonce Số thứ tự tin nhắn
     * @param _payload Dữ liệu được gửi từ chain nguồn
     */
    function lzReceive(uint16 _srcChainId, bytes calldata _srcAddress, uint64 _nonce, bytes calldata _payload) external;
} 