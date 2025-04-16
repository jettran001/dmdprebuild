// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

/**
 * @dev Interface cho LayerZero Endpoint.
 * Đây là interface đơn giản cho việc tương tác với LayerZero
 */
interface ILayerZeroEndpoint {
    // Gửi một message đến chain khác thông qua LayerZero
    function send(
        uint16 _dstChainId,
        bytes calldata _destination,
        bytes calldata _payload,
        address payable _refundAddress,
        address _zroPaymentAddress,
        bytes calldata _adapterParams
    ) external payable;

    // Ước tính phí cho việc gửi message
    function estimateFees(
        uint16 _dstChainId,
        address _userApplication,
        bytes calldata _payload,
        bool _payInZRO,
        bytes calldata _adapterParam
    ) external view returns (uint256 nativeFee, uint256 zroFee);

    // Nhận message từ chain khác
    function receivePayload(
        uint16 _srcChainId,
        bytes calldata _srcAddress,
        address _dstAddress,
        uint64 _nonce,
        uint _gasLimit,
        bytes calldata _payload
    ) external;

    // Lấy gas limit mặc định cho một chain đích
    function getInboundNonce(uint16 _srcChainId, bytes calldata _srcAddress) external view returns (uint64);

    // Lấy outbound nonce cho một chain đích
    function getOutboundNonce(uint16 _dstChainId, address _srcAddress) external view returns (uint64);

    // Lấy ChainId của chain hiện tại
    function getChainId() external view returns (uint16);

    // Lấy version hiện tại
    function getConfig(
        uint16 _version,
        uint16 _chainId,
        address _userApplication,
        uint _configType
    ) external view returns (bytes memory);

    // Kiểm tra xem endpoint có hỗ trợ chain đích không
    function getSendVersion(address _userApplication) external view returns (uint16);

    // Kiểm tra xem endpoint có hỗ trợ chain nguồn không
    function getReceiveVersion(address _userApplication) external view returns (uint16);
} 