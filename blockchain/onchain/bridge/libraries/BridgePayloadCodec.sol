// SPDX-License-Identifier: MIT
pragma solidity 0.8.20;

import "../interfaces/BridgeTypes.sol";

/**
 * @title BridgePayloadCodec
 * @dev Library for encoding and decoding BridgePayload with version and checksum (standardized by .solguard)
 */
library BridgePayloadCodec {
    uint8 public constant PAYLOAD_VERSION = 1;
    uint16 public constant MAX_DESTINATION_LENGTH = 128;
    uint16 public constant MAX_EXTRA_DATA_LENGTH = 256;

    /**
     * @notice Encode BridgePayload to bytes (with version and checksum)
     * @param payload BridgePayload struct
     * @return Encoded bytes
     */
    function encode(BridgePayload memory payload) internal pure returns (bytes memory) {
        bytes memory data = abi.encode(
            PAYLOAD_VERSION,
            payload.sender,
            payload.destination,
            payload.amount,
            payload.timestamp,
            payload.token,
            payload.tokenType,
            payload.unwrapper,
            payload.needUnwrap,
            payload.nonce,
            payload.extraData
        );
        bytes32 checksum = keccak256(data);
        return abi.encodePacked(data, checksum);
    }

    function validateExtraData(bytes memory extraData) internal pure {
        require(extraData.length <= MAX_EXTRA_DATA_LENGTH, "BridgePayloadCodec: extraData too long");
        // Add more validation logic if needed
    }

    /**
     * @notice Decode bytes to BridgePayload, check version and checksum
     * @param encoded Encoded bytes
     * @return Decoded BridgePayload struct
     */
    function decode(bytes memory encoded) internal pure returns (BridgePayload memory) {
        require(encoded.length >= 288, "BridgePayloadCodec: payload too short");
        bytes memory data = new bytes(encoded.length - 32);
        bytes32 checksum;
        // Safe copy instead of assembly
        for (uint i = 0; i < data.length; i++) {
            data[i] = encoded[i];
        }
        checksum = bytes32(encoded[encoded.length - 32:encoded.length]);
        require(keccak256(data) == checksum, "BridgePayloadCodec: invalid checksum");
        (
            uint8 version,
            address sender,
            bytes memory destination,
            uint256 amount,
            uint256 timestamp,
            address token,
            uint8 tokenType,
            address unwrapper,
            bool needUnwrap,
            uint64 nonce,
            bytes memory extraData
        ) = abi.decode(data, (uint8, address, bytes, uint256, uint256, address, uint8, address, bool, uint64, bytes));
        require(version == PAYLOAD_VERSION, "BridgePayloadCodec: version mismatch");
        require(destination.length <= MAX_DESTINATION_LENGTH, "BridgePayloadCodec: destination too long");
        if (extraData.length > 0) {
            validateExtraData(extraData);
        }
        return BridgePayload({
            version: version,
            sender: sender,
            destination: destination,
            amount: amount,
            timestamp: timestamp,
            token: token,
            tokenType: tokenType,
            unwrapper: unwrapper,
            needUnwrap: needUnwrap,
            nonce: nonce,
            extraData: extraData,
            checksum: checksum
        });
    }
} 