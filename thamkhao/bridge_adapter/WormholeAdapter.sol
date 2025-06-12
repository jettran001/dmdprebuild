// SPDX-License-Identifier: MIT
pragma solidity 0.8.20;

import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/security/ReentrancyGuard.sol";
import "@openzeppelin/contracts/utils/cryptography/ECDSA.sol";
import "@openzeppelin/contracts/access/AccessControl.sol";
import "@openzeppelin/contracts/security/Pausable.sol";
import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import "./IBridgeAdapter.sol";
import "../bridge_interface.sol";

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
    
    function parseAndVerifyVM(bytes memory encodedVM) 
        external view returns (
            address emitterAddress,
            uint16 emitterChainId,
            bytes memory payload,
            uint64 sequence,
            uint8 consistencyLevel
        );
}

/**
 * @title WormholeAdapter
 * @dev Adapter đơn giản kết nối với Wormhole protocol cho bridging
 * Tập trung vào vai trò adapter "plugin" cho bridge_interface.sol
 */
contract WormholeAdapter is IBridgeAdapter, Ownable, ReentrancyGuard, AccessControl, Pausable {
    using ECDSA for bytes32;
    using SafeERC20 for IERC20;

    // ================= Constants =================
    string private constant ADAPTER_TYPE = "Wormhole";
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
    mapping(uint16 => bytes32) public trustedRemotes; // chainId => remote emitter address (bytes32)
    
    // Wormhole configuration
    address public wormhole;
    uint8 public consistencyLevel = 15; // finality level

    // Token address
    address private _tokenAddress;
    
    // Bridge limits
    uint256 public limitPerTx;         // Maximum token amount per transaction
    uint256 public limitPerPeriod;     // Maximum token amount per time period
    uint256 public periodDuration = 1 days;
    uint256 public maxDailyVolume;     // Daily volume limit
    
    // Rate limiting tracking
    mapping(uint256 => uint256) public periodVolumes; // period => volume
    
    // Message tracking to prevent double processing
    mapping(bytes32 => bool) public processedMessages;
    
    // ================= Events =================
    event WormholeAddressUpdated(address indexed previousAddress, address indexed newAddress);
    event ConsistencyLevelUpdated(uint8 previousLevel, uint8 newLevel);
    event TrustedRemoteSet(uint16 indexed chainId, bytes32 remoteAddress);
    event LimitUpdated(uint256 limitPerTx, uint256 limitPerPeriod, uint256 periodDuration, uint256 maxDailyVolume);
    event ChainNameSet(uint16 indexed chainId, string name);
    event MessageSent(uint16 dstChainId, uint64 sequence, bytes payload);
    event MessageReceived(uint16 srcChainId, bytes32 srcAddress, uint64 sequence, bytes payload);
    event BridgeInitiated(address indexed sender, uint16 dstChainId, bytes destination, uint256 amount);
    event BridgeCompleted(uint16 srcChainId, address indexed receiver, uint256 amount);
    event BridgeFailed(address indexed sender, uint16 dstChainId, bytes destination, uint256 amount, string reason);
    event FeeRefunded(address indexed to, uint256 amount);
    event EmergencyTokenWithdrawn(address token, address to, uint256 amount, uint256 timestamp);
    
    /**
     * @dev Initialize WormholeAdapter
     * @param _wormhole Address of the Wormhole core contract
     * @param targetChainId ID of the target chain
     * @param supportedChainIds List of supported chain IDs
     * @param _limitPerTx Maximum token amount per transaction
     * @param _limitPerPeriod Maximum token amount per time period
     * @param _maxDailyVolume Maximum daily volume limit
     * @param tokenAddr Token address
     */
    constructor(
        address _wormhole,
        uint16 targetChainId,
        uint16[] memory supportedChainIds,
        uint256 _limitPerTx,
        uint256 _limitPerPeriod,
        uint256 _maxDailyVolume,
        address tokenAddr
    ) Ownable(msg.sender) {
        require(_wormhole != address(0), "WormholeAdapter: Invalid Wormhole address");
        require(tokenAddr != address(0), "WormholeAdapter: Invalid token address");
        require(supportedChainIds.length > 0, "WormholeAdapter: No supported chains");
        
        wormhole = _wormhole;
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
        chainNames[2] = "Solana";
        chainNames[4] = "BSC";
        chainNames[5] = "Polygon";
        chainNames[6] = "Avalanche";
        chainNames[10] = "Fantom";
        chainNames[11] = "Celo";
        chainNames[14] = "Moonbeam";
        chainNames[16] = "Optimism";
        chainNames[23] = "Arbitrum";
        chainNames[22] = "Aptos";
        chainNames[28] = "NEAR";
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
     * @dev Bridge tokens to target chain via Wormhole
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
        require(isChainSupported(dstChainId), "WormholeAdapter: Chain not supported");
        require(trustedRemotes[dstChainId] != 0, "WormholeAdapter: Trusted remote not set");
        require(amount > 0, "WormholeAdapter: Amount must be greater than 0");
        require(amount <= limitPerTx, "WormholeAdapter: Amount exceeds limitPerTx");
        require(destination.length > 0, "WormholeAdapter: Empty destination");
        require(destination.length <= MAX_PAYLOAD_SIZE, "WormholeAdapter: Destination too large");
        
        // Check bridge limits
        require(_checkAndUpdatePeriodVolume(amount), "WormholeAdapter: Volume limit exceeded");
        
        // Calculate Wormhole message fee
        uint256 wormholeFee = this.estimateFee(dstChainId, amount);
        require(msg.value >= wormholeFee, "WormholeAdapter: Insufficient fee");
        
        // Transfer tokens from user to this contract
        IERC20(_tokenAddress).safeTransferFrom(msg.sender, address(this), amount);
        
        // Prepare bridge payload
        bytes memory payload = abi.encode(
            msg.sender,         // Source address
            destination,        // Destination address
            amount,             // Amount
            block.timestamp     // Timestamp
        );
        
        // Send Wormhole message
        uint64 sequence = IWormhole(wormhole).publishMessage{value: wormholeFee}(
            0,                  // nonce (0 means use auto-incrementing sequence)
            payload,
            consistencyLevel
        );
        
        // Emit events
        emit MessageSent(dstChainId, sequence, payload);
        emit BridgeInitiated(msg.sender, dstChainId, destination, amount);
        
        // Refund excess fee if any
        uint256 excessFee = msg.value - wormholeFee;
        if (excessFee > 0) {
            (bool success, ) = payable(msg.sender).call{value: excessFee}("");
            require(success, "WormholeAdapter: Fee refund failed");
            emit FeeRefunded(msg.sender, excessFee);
        }
    }
    
    /**
     * @dev Process a received Wormhole message
     * @param encodedVM Encoded Wormhole message
     */
    function receiveMessage(bytes calldata encodedVM) external nonReentrant whenNotPaused {
        // Parse and verify the Wormhole message
        (
            address emitterAddress,
            uint16 srcChainId,
            bytes memory payload,
            uint64 sequence,
            uint8 _consistencyLevel
        ) = IWormhole(wormhole).parseAndVerifyVM(encodedVM);
        
        // Convert emitter address to bytes32 for comparison
        bytes32 emitterAddressBytes32 = bytes32(uint256(uint160(emitterAddress)));
        
        // Verify trusted remote
        require(_isSupportedChain[srcChainId], "WormholeAdapter: Unsupported source chain");
        require(trustedRemotes[srcChainId] == emitterAddressBytes32, "WormholeAdapter: Invalid source address");
        
        // Create message ID to prevent double processing
        bytes32 messageId = keccak256(abi.encodePacked(srcChainId, emitterAddressBytes32, sequence));
        require(!processedMessages[messageId], "WormholeAdapter: Message already processed");
        processedMessages[messageId] = true;
        
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
        
        // Emit message received event
        emit MessageReceived(srcChainId, emitterAddressBytes32, sequence, payload);
        
        // Transfer tokens to destination
        try IERC20(_tokenAddress).safeTransfer(destAddress, amount) {
            // Successfully transferred
            emit BridgeCompleted(srcChainId, destAddress, amount);
        } catch (bytes memory reason) {
            // Handle transfer failure
            emit BridgeFailed(srcSender, srcChainId, destAddressBytes, amount, string(reason));
        }
    }
    
    /**
     * @dev Set trusted remote for chain
     * @param chainId Chain ID
     * @param remoteAddress Remote emitter address as bytes32
     */
    function setTrustedRemote(uint16 chainId, bytes32 remoteAddress) external onlyRole(ADMIN_ROLE) {
        require(remoteAddress != 0, "WormholeAdapter: Empty remote address");
        trustedRemotes[chainId] = remoteAddress;
        emit TrustedRemoteSet(chainId, remoteAddress);
    }
    
    /**
     * @dev Set chain name
     * @param chainId Chain ID
     * @param name Chain name
     */
    function setChainName(uint16 chainId, string calldata name) external onlyRole(ADMIN_ROLE) {
        require(bytes(name).length > 0, "WormholeAdapter: Empty chain name");
        chainNames[chainId] = name;
        emit ChainNameSet(chainId, name);
    }
    
    /**
     * @dev Update Wormhole contract address
     * @param newWormhole New Wormhole contract address
     */
    function updateWormholeAddress(address newWormhole) external onlyRole(ADMIN_ROLE) {
        require(newWormhole != address(0), "WormholeAdapter: Invalid Wormhole address");
        address previousWormhole = wormhole;
        wormhole = newWormhole;
        emit WormholeAddressUpdated(previousWormhole, newWormhole);
    }
    
    /**
     * @dev Update consistency level
     * @param newLevel New consistency level
     */
    function updateConsistencyLevel(uint8 newLevel) external onlyRole(ADMIN_ROLE) {
        require(newLevel > 0, "WormholeAdapter: Invalid consistency level");
        uint8 previousLevel = consistencyLevel;
        consistencyLevel = newLevel;
        emit ConsistencyLevelUpdated(previousLevel, newLevel);
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
        require(_limitPerTx > 0, "WormholeAdapter: Invalid limitPerTx");
        require(_limitPerPeriod > 0, "WormholeAdapter: Invalid limitPerPeriod");
        require(_periodDuration > 0, "WormholeAdapter: Invalid periodDuration");
        require(_maxDailyVolume > 0, "WormholeAdapter: Invalid maxDailyVolume");
        
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
        require(isChainSupported(dstChainId), "WormholeAdapter: Chain not supported");
        
        // Wormhole có phí cố định cho mỗi message
        return 0.001 ether; // Ví dụ, thực tế cần truy vấn từ Wormhole contract
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
        require(to != address(0), "WormholeAdapter: Invalid recipient");
        require(amount > 0, "WormholeAdapter: Invalid amount");
        
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
        require(bytes(name).length > 0, "WormholeAdapter: Chain name not set");
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
}
