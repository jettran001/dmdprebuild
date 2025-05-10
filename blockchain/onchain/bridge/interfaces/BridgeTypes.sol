// SPDX-License-Identifier: MIT
pragma solidity 0.8.20;

/**
 * @title BridgeTypes
 * @dev Unified types, enums, and events for cross-chain bridge system (standardized by .solguard)
 */

/// @notice Status of a bridge operation
enum BridgeStatus {
    Pending,
    Completed,
    Failed,
    Retrying,
    Expired,
    Unwrapping,
    ChecksumFailed
}

/// @notice Payload structure for cross-chain bridge messages
struct BridgePayload {
    uint8 version; // Payload version for compatibility
    address sender;
    bytes destination;
    uint256 amount;
    uint256 timestamp;
    address token;
    uint8 tokenType; // 1=ERC20, 2=ERC1155, 3=Native, 4=ERC721, 5=ERC777
    address unwrapper; // Unwrapper contract (if auto-unwrap needed)
    bool needUnwrap;
    uint64 nonce;
    bytes extraData; // For future compatibility
    bytes32 checksum; // For integrity check
}

/// @notice Validate integrity of BridgePayload (checksum)
function validatePayloadIntegrity(BridgePayload memory payload) pure returns (bool) {
    bytes memory data = abi.encode(
        payload.version,
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
    return keccak256(data) == payload.checksum;
}

/// @notice Event emitted when checksum validation fails
event ChecksumValidationFailed(
    bytes32 indexed bridgeId,
    address indexed sender,
    address indexed token,
    uint16 destinationChainId,
    bytes destinationAddress,
    uint256 amount,
    uint256 timestamp
);

/// @notice Bridge operation tracking structure
struct BridgeOperation {
    bytes32 bridgeId;
    address sender;
    address token;
    uint16 destinationChainId;
    bytes destinationAddress;
    uint256 amount;
    uint256 timestamp;
    BridgeStatus status;
    bool completed;
    bytes32 relatedOpId;
}

/// @notice Stuck token tracking structure
struct StuckToken {
    address owner;
    address token;
    uint256 amount;
    uint16 dstChainId;
    bytes recipient;
    uint256 timestamp;
    bool rescued;
}

// --- Standardized Events ---
event BridgeOperationCreated(
    bytes32 indexed bridgeId,
    address indexed sender,
    address indexed token,
    uint16 destinationChainId,
    bytes destinationAddress,
    uint256 amount,
    BridgeStatus status,
    uint256 timestamp
);

event BridgeOperationUpdated(
    bytes32 indexed bridgeId,
    BridgeStatus status,
    bool completed,
    uint256 timestamp
);

event TokenWrapped(
    bytes32 indexed bridgeId,
    address indexed sender,
    address indexed token,
    uint256 amount,
    uint256 timestamp
);

event TokenUnwrapped(
    bytes32 indexed bridgeId,
    address indexed sender,
    address indexed token,
    uint256 amount,
    uint256 timestamp
);

event AutoUnwrapProcessed(
    bytes32 indexed bridgeId,
    address indexed user,
    address indexed unwrapper,
    uint256 amount,
    bool success,
    uint256 timestamp
);

event StuckTokenRegistered(
    bytes32 indexed stuckId,
    address indexed owner,
    address indexed token,
    uint16 dstChainId,
    uint256 amount,
    uint256 timestamp
);

event StuckTokenRescued(
    bytes32 indexed stuckId,
    address indexed owner,
    address indexed token,
    uint256 amount,
    uint256 timestamp
); 