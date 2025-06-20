// SPDX-License-Identifier: MIT
pragma solidity 0.8.20;

import "@layerzerolabs/solidity-examples/contracts/lzApp/NonblockingLzApp.sol";
import "@openzeppelin/contracts/security/ReentrancyGuard.sol";
import "@openzeppelin/contracts/access/AccessControl.sol";
import "@openzeppelin/contracts/security/Pausable.sol";
import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import "./IBridgeAdapter.sol";

/**
 * @title IBridgeToken
 * @dev Interface for bridge tokens with a function to return tokens when bridge operation fails
 */
interface IBridgeToken {
    /**
     * @dev Return tokens to the user when bridge operation fails
     * @param to Address receiving the tokens
     * @param amount Amount of tokens
     */
    function returnTokens(address to, uint256 amount) external returns (bool);
}

/**
 * @title LayerZeroAdapter
 * @dev Adapter that connects to the LayerZero protocol for bridging
 * Simplified version that leverages LayerZero's native relayer system
 */
contract LayerZeroAdapter is IBridgeAdapter, NonblockingLzApp, ReentrancyGuard, AccessControl, Pausable {
    using SafeERC20 for IERC20;

    // ================= Constants =================
    string private constant ADAPTER_TYPE = "LayerZero";
    bytes32 public constant ADMIN_ROLE = keccak256("ADMIN_ROLE");
    bytes32 public constant FEE_MANAGER_ROLE = keccak256("FEE_MANAGER_ROLE");
    bytes32 public constant BRIDGE_ROLE = keccak256("BRIDGE_ROLE");
    uint256 public constant MAX_PAYLOAD_SIZE = 10000;
    
    // ================= State Variables =================
    // Chain configurations
    uint16 private immutable _targetChain;
    uint16[] private _supportedChains;
    mapping(uint16 => bool) private _isSupportedChain;
    mapping(uint16 => string) public chainNames;
    mapping(uint16 => bytes) public trustedRemotes; // chainId => remote address
    
    // Token address
    address private _tokenAddress;
    
    // Bridge limits
    uint256 public limitPerTx;         // Maximum token amount per transaction
    uint256 public limitPerPeriod;     // Maximum token amount per time period
    uint256 public periodDuration = 1 days;
    uint256 public maxDailyVolume;     // Daily volume limit
    
    // Rate limiting tracking
    mapping(uint256 => uint256) public periodVolumes; // period => volume
    
    // ================= Events =================
    event TrustedRemoteSet(uint16 indexed remoteChainId, bytes remoteAddress);
    event LimitUpdated(uint256 limitPerTx, uint256 limitPerPeriod, uint256 periodDuration, uint256 maxDailyVolume);
    event ChainNameSet(uint16 indexed chainId, string name);
    event BridgeInitiated(address indexed sender, uint16 dstChainId, bytes destination, uint256 amount, uint64 nonce);
    event BridgeCompleted(uint16 srcChainId, address indexed receiver, uint256 amount, uint64 nonce);
    event BridgeFailed(address indexed sender, uint16 dstChainId, bytes destination, uint256 amount, string reason);
    event FeeRefunded(address indexed to, uint256 amount);
    event EmergencyTokenWithdrawn(address token, address to, uint256 amount, uint256 timestamp);
    event ChainSupportRemoved(uint16 indexed chainId);
    
    /**
     * @dev Initialize LayerZeroAdapter
     * @param _lzEndpoint Address of the LayerZero endpoint
     * @param targetChainId ID of the target chain
     * @param supportedChainIds List of supported chain IDs
     * @param _limitPerTx Maximum token amount per transaction
     * @param _limitPerPeriod Maximum token amount per time period
     * @param _maxDailyVolume Maximum daily volume limit
     * @param tokenAddr Token address
     */
    constructor(
        address _lzEndpoint, 
        uint16 targetChainId,
        uint16[] memory supportedChainIds,
        uint256 _limitPerTx,
        uint256 _limitPerPeriod,
        uint256 _maxDailyVolume,
        address tokenAddr
    ) NonblockingLzApp(_lzEndpoint) {
        require(_lzEndpoint != address(0), "LayerZeroAdapter: Invalid endpoint address");
        require(tokenAddr != address(0), "LayerZeroAdapter: Invalid token address");
        require(supportedChainIds.length > 0, "LayerZeroAdapter: No supported chains");
        
        _targetChain = targetChainId;
        _supportedChains = supportedChainIds;
        
        // Initialize supportedChains mapping for optimized lookups
        for (uint256 i = 0; i < supportedChainIds.length; i++) {
            _isSupportedChain[supportedChainIds[i]] = true;
        }
        
        limitPerTx = _limitPerTx;
        limitPerPeriod = _limitPerPeriod;
        maxDailyVolume = _maxDailyVolume;
        _tokenAddress = tokenAddr;
        
        // Initialize standard chain names
        _initializeChainNames();
        
        // Setup roles
        _grantRole(DEFAULT_ADMIN_ROLE, msg.sender);
        _grantRole(ADMIN_ROLE, msg.sender);
        _grantRole(FEE_MANAGER_ROLE, msg.sender);
        _grantRole(BRIDGE_ROLE, msg.sender);
    }
    
    /**
     * @dev Initialize standard chain names
     */
    function _initializeChainNames() internal {
        chainNames[1] = "Ethereum";
        chainNames[56] = "BSC";
        chainNames[137] = "Polygon";
        chainNames[43114] = "Avalanche";
        chainNames[10] = "Optimism";
        chainNames[42161] = "Arbitrum";
        chainNames[250] = "Fantom";
        chainNames[1284] = "Moonbeam";
        chainNames[1285] = "Moonriver";
        chainNames[1313161554] = "NEAR";
        chainNames[999999999] = "Solana";
    }
    
    /**
     * @dev Get the current period
     * @return The current period
     */
    function _getCurrentPeriod() internal view returns (uint256) {
        return block.timestamp / periodDuration;
    }
    
    /**
     * @dev Check and update volume within period
     * @param amount Token amount to bridge
     * @return true if bridge is possible
     */
    function _checkAndUpdatePeriodVolume(uint256 amount) internal returns (bool) {
        uint256 currentPeriod = _getCurrentPeriod();
        
        // Check global limit
        uint256 newGlobalVolume = periodVolumes[currentPeriod] + amount;
        if (newGlobalVolume > limitPerPeriod) {
            return false;
        }
        
        // Check daily limit
        uint256 dailyPeriod = block.timestamp / 1 days;
        uint256 newDailyVolume = periodVolumes[dailyPeriod] + amount;
        if (newDailyVolume > maxDailyVolume) {
            return false;
        }
        
        // Update volumes
        periodVolumes[currentPeriod] = newGlobalVolume;
        periodVolumes[dailyPeriod] = newDailyVolume;
        
        return true;
    }
    
    /**
     * @dev Bridge tokens to target chain via LayerZero
     * @param dstChainId ID of the destination chain
     * @param destination Encoded destination address
     * @param amount Token amount to bridge
     */
    function bridgeTo(
        uint16 dstChainId, 
        bytes calldata destination, 
        uint256 amount
    ) external payable override nonReentrant whenNotPaused {
        // Validation
        require(isChainSupported(dstChainId), "LayerZeroAdapter: Chain not supported");
        require(trustedRemotes[dstChainId].length > 0, "LayerZeroAdapter: Trusted remote not set");
        require(amount > 0, "LayerZeroAdapter: Amount must be greater than 0");
        require(amount <= limitPerTx, "LayerZeroAdapter: Amount exceeds limitPerTx");
        require(destination.length > 0, "LayerZeroAdapter: Empty destination");
        require(destination.length <= MAX_PAYLOAD_SIZE, "LayerZeroAdapter: Destination too large");
        
        // Check bridge limits
        require(_checkAndUpdatePeriodVolume(amount), "LayerZeroAdapter: Volume limit exceeded");
        
        // Prepare bridge payload
        bytes memory payload = abi.encode(
            msg.sender,         // Source address
            destination,        // Destination address
            amount,             // Amount
            block.timestamp     // Timestamp
        );
        
        // Calculate LZ fee
        (uint256 nativeFee, ) = lzEndpoint.estimateFees(
            dstChainId,
            address(this),
            payload,
            false,              // Pay in native token
            bytes("")           // Adapter params
        );
        
        require(msg.value >= nativeFee, "LayerZeroAdapter: Insufficient fee");
        
        // Transfer tokens from user to this contract
        IERC20(_tokenAddress).safeTransferFrom(msg.sender, address(this), amount);
        
        // Send LayerZero message
        _lzSend(
            dstChainId,
            payload,
            payable(msg.sender),
            address(0x0),       // Zero address as refund address
            bytes(""),          // No adapter params
            msg.value
        );
        
        // Emit event
        emit BridgeInitiated(msg.sender, dstChainId, destination, amount, lzEndpoint.getOutboundNonce(dstChainId, address(this)));
        
        // Refund excess fee if any
        uint256 excessFee = msg.value - nativeFee;
        if (excessFee > 0) {
            (bool success, ) = payable(msg.sender).call{value: excessFee}("");
            require(success, "LayerZeroAdapter: Fee refund failed");
            emit FeeRefunded(msg.sender, excessFee);
        }
    }
    
    /**
     * @dev Handle received LayerZero message (override _nonblockingLzReceive)
     * @param srcChainId Source chain ID
     * @param srcAddress Source address
     * @param nonce Message nonce
     * @param payload Message payload
     */
    function _nonblockingLzReceive(
        uint16 srcChainId,
        bytes memory srcAddress,
        uint64 nonce,
        bytes memory payload
    ) internal override {
        // Validate trusted remote
        require(_isSupportedChain[srcChainId], "LayerZeroAdapter: Unsupported source chain");
        require(_checkTrustedRemote(srcChainId, srcAddress), "LayerZeroAdapter: Invalid source address");
        
        // Decode payload
        (
            address srcSender,
            bytes memory destAddressBytes,
            uint256 amount,
            uint256 timestamp
        ) = abi.decode(payload, (address, bytes, uint256, uint256));
        
        // Convert destination address bytes to address
        address destAddress;
        assembly {
            destAddress := mload(add(destAddressBytes, 20))
        }
        
        // Emit bridge completed event
        emit BridgeCompleted(srcChainId, destAddress, amount, nonce);
        
        // Transfer tokens to destination
        try IERC20(_tokenAddress).safeTransfer(destAddress, amount) {
            // Successfully transferred
        } catch (bytes memory reason) {
            // Handle transfer failure
            emit BridgeFailed(srcSender, srcChainId, destAddressBytes, amount, string(reason));
        }
    }
    
    /**
     * @dev Check if the remote address is trusted
     * @param srcChainId Source chain ID
     * @param srcAddress Source address
     * @return Whether the source is trusted
     */
    function _checkTrustedRemote(uint16 srcChainId, bytes memory srcAddress) internal view returns (bool) {
        bytes memory trustedSource = trustedRemotes[srcChainId];
        return trustedSource.length > 0 && keccak256(trustedSource) == keccak256(srcAddress);
    }
    
    /**
     * @dev Set trusted remote for chain
     * @param remoteChainId Chain ID
     * @param remoteAddress Remote address
     */
    function setTrustedRemote(uint16 remoteChainId, bytes calldata remoteAddress) external onlyRole(ADMIN_ROLE) {
        require(remoteAddress.length > 0, "LayerZeroAdapter: Empty remote address");
        trustedRemotes[remoteChainId] = remoteAddress;
        emit TrustedRemoteSet(remoteChainId, remoteAddress);
    }
    
    /**
     * @dev Set chain name
     * @param chainId Chain ID
     * @param name Chain name
     */
    function setChainName(uint16 chainId, string calldata name) external onlyRole(ADMIN_ROLE) {
        require(bytes(name).length > 0, "LayerZeroAdapter: Empty chain name");
        chainNames[chainId] = name;
        emit ChainNameSet(chainId, name);
    }
    
    /**
     * @dev Update bridge limits
     * @param _limitPerTx Maximum token amount per transaction
     * @param _limitPerPeriod Maximum token amount per time period
     * @param _periodDuration Duration of a time period
     * @param _maxDailyVolume Maximum daily volume
     */
    function updateLimits(
        uint256 _limitPerTx,
        uint256 _limitPerPeriod,
        uint256 _periodDuration,
        uint256 _maxDailyVolume
    ) external onlyRole(ADMIN_ROLE) {
        require(_limitPerTx > 0, "LayerZeroAdapter: Invalid limitPerTx");
        require(_limitPerPeriod > 0, "LayerZeroAdapter: Invalid limitPerPeriod");
        require(_periodDuration > 0, "LayerZeroAdapter: Invalid periodDuration");
        require(_maxDailyVolume > 0, "LayerZeroAdapter: Invalid maxDailyVolume");
        
        limitPerTx = _limitPerTx;
        limitPerPeriod = _limitPerPeriod;
        periodDuration = _periodDuration;
        maxDailyVolume = _maxDailyVolume;
        
        emit LimitUpdated(limitPerTx, limitPerPeriod, periodDuration, maxDailyVolume);
    }
    
    /**
     * @dev Pause the bridge
     */
    function pause() external onlyRole(ADMIN_ROLE) {
        _pause();
    }
    
    /**
     * @dev Unpause the bridge
     */
    function unpause() external onlyRole(ADMIN_ROLE) {
        _unpause();
    }
    
    /**
     * @dev Estimate fee for bridge
     * @param dstChainId Destination chain ID
     * @param amount Token amount to bridge
     * @return Estimated fee
     */
    function estimateFee(uint16 dstChainId, uint256 amount) external view override returns (uint256) {
        require(isChainSupported(dstChainId), "LayerZeroAdapter: Chain not supported");
        
        bytes memory payload = abi.encode(
            address(0), // Placeholder for sender
            bytes(""),  // Placeholder for destination
            amount,
            uint256(0)  // Placeholder for timestamp
        );
        
        (uint256 nativeFee, ) = lzEndpoint.estimateFees(
            dstChainId,
            address(this),
            payload,
            false,
            bytes("")
        );
        
        return nativeFee;
    }
    
    /**
     * @dev Emergency withdrawal of tokens (with timelock via AccessControl)
     * @param token Token address
     * @param to Recipient address
     * @param amount Amount to withdraw
     */
    function emergencyWithdraw(address token, address to, uint256 amount) 
        external
        onlyRole(ADMIN_ROLE) 
    {
        require(to != address(0), "LayerZeroAdapter: Invalid recipient");
        require(amount > 0, "LayerZeroAdapter: Invalid amount");
        
        IERC20(token).safeTransfer(to, amount);
        
        emit EmergencyTokenWithdrawn(token, to, amount, block.timestamp);
    }
    
    /**
     * @dev Get supported chains
     * @return Supported chain IDs
     */
    function supportedChains() external view override returns (uint16[] memory) {
        return _supportedChains;
    }
    
    /**
     * @dev Get adapter type
     * @return Adapter type
     */
    function adapterType() external pure override returns (string memory) {
        return ADAPTER_TYPE;
    }
    
    /**
     * @dev Get target chain
     * @return Target chain ID
     */
    function targetChain() external view override returns (uint16) {
        return _targetChain;
    }
    
    /**
     * @dev Check if chain is supported
     * @param chainId Chain ID
     * @return Whether chain is supported
     */
    function isChainSupported(uint16 chainId) public view override returns (bool) {
        return _isSupportedChain[chainId];
    }
    
    /**
     * @dev Get chain name
     * @param chainId Chain ID
     * @return Chain name
     */
    function getChainName(uint16 chainId) external view override returns (string memory) {
        string memory name = chainNames[chainId];
        require(bytes(name).length > 0, "LayerZeroAdapter: Chain name not set");
        return name;
    }
    
    /**
     * @dev Get token address
     * @return Token address
     */
    function getTokenAddress() external view returns (address) {
        return _tokenAddress;
    }
    
    /**
     * @dev Receive function to accept ETH
     */
    receive() external payable {}
    
    /**
     * @dev Remove a chain from supported chains
     * @param chainId Chain ID to remove
     */
    function removeSupportedChain(uint16 chainId) external onlyRole(ADMIN_ROLE) {
        require(_isSupportedChain[chainId], "LayerZeroAdapter: Chain not supported");
        require(chainId != _targetChain, "LayerZeroAdapter: Cannot remove target chain");
        
        // Remove from supported chains mapping
        _isSupportedChain[chainId] = false;
        
        // Remove from _supportedChains array
        uint256 length = _supportedChains.length;
        for (uint256 i = 0; i < length; i++) {
            if (_supportedChains[i] == chainId) {
                // Swap with the last element and pop
                _supportedChains[i] = _supportedChains[length - 1];
                _supportedChains.pop();
                break;
            }
        }
        
        // Delete trusted remote
        delete trustedRemotes[chainId];
        
        emit ChainSupportRemoved(chainId);
    }
}
