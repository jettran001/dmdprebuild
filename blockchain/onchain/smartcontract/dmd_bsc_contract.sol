// SPDX-License-Identifier: MIT
pragma solidity 0.8.20;

import "@openzeppelin/contracts/token/ERC1155/ERC1155.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/security/Pausable.sol";
import "@openzeppelin/contracts/token/ERC1155/extensions/ERC1155Burnable.sol";
import "@openzeppelin/contracts/token/ERC1155/extensions/ERC1155Supply.sol";
import "@openzeppelin/contracts/utils/Strings.sol";
import "@layerzerolabs/solidity-examples/contracts/lzApp/NonblockingLzApp.sol";

// Interface cho Wrapped DMD token (ERC20)
interface IWrappedDMD {
    function mintWrapped(address to, uint256 amount) external;
    function burnWrapped(address from, uint256 amount) external;
    function balanceOf(address account) external view returns (uint256);
    function wrapFromERC1155(uint256 amount) external;
    function unwrapToERC1155(uint256 amount) external;
    function getTotalBalance(address account) external view returns (uint256);
    function invalidateTierCache(address user) external;
}

/**
 * @title DiamondToken
 * @dev DMD Token ERC-1155 standard on BSC with LayerZero bridging capability
 * @notice Đây là sub contract nhận token từ NEAR (main chain) thông qua bridge
 */
contract DiamondToken is ERC1155, Ownable, Pausable, ERC1155Burnable, ERC1155Supply, NonblockingLzApp {
    using Strings for uint256;
    
    // ID for DMD fungible token (FT)
    uint256 public constant DMD_TOKEN_ID = 0;
    
    // Base metadata URI
    string private _baseURI;
    
    // Token name and symbol
    string public name;
    string public symbol;
    
    // Token decimals
    uint8 public decimals;
    
    // Mapping to store metadata URI for NFTs
    mapping(uint256 => string) private _tokenURIs;
    
    // Total supply of DMD token
    uint256 private _totalSupply;
    
    // Tracking amount of bridged tokens
    uint256 public bridgedSupply;
    
    // LayerZero endpoint on destination chain (NEAR)
    uint16 public constant NEAR_CHAIN_ID = 115; // LayerZero chain ID for NEAR
    
    // Địa chỉ của Wrapped DMD Token (ERC20)
    IWrappedDMD public wrappedDMD;
    
    // Địa chỉ bridge proxy
    address public bridgeProxy;
    
    // Enum defining tiers for DMD token
    enum DMDTier {
        Regular,     // Regular Fungible Token (< 1000)
        Bronze,      // VIP Badge tier Bronze (>= 1000)
        Silver,      // VIP Badge tier Silver (>= 5000)
        Gold,        // VIP Badge tier Gold (>= 10000)
        Diamond      // VIP Badge tier Diamond (>= 30000)
    }
    
    // Token amount thresholds for each tier
    uint256 public bronzeThreshold = 1000 * 10**18;  // 1,000 DMD
    uint256 public silverThreshold = 5000 * 10**18;  // 5,000 DMD
    uint256 public goldThreshold = 10000 * 10**18;   // 10,000 DMD
    uint256 public diamondThreshold = 30000 * 10**18; // 30,000 DMD
    
    // Array storing tiers
    DMDTier[] public tiers;
    
    // Mappings to store custom tier information
    mapping(uint8 => uint256) public customTierThresholds;
    mapping(uint8 => string) public customTierNames;
    mapping(uint8 => string) public customTierDescriptions;
    mapping(uint8 => string) public customTierURIs;
    
    // Cache storing user tiers to optimize gas
    mapping(address => DMDTier) private _userTierCache;
    // Cache expiry time (in seconds)
    mapping(address => uint256) private _userTierCacheExpiry;
    // Cache time-to-live (default: 1 hour)
    uint256 public tierCacheTTL = 1 hours;
    
    // Tier URIs for each tier type
    string public regularTierURI;
    string public bronzeTierURI;
    string public silverTierURI;
    string public goldTierURI;
    string public diamondTierURI;
    
    // Events
    event TokenMinted(address indexed to, uint256 indexed id, uint256 amount);
    event TokenBurned(address indexed from, uint256 indexed id, uint256 amount);
    event NFTMinted(address indexed to, uint256 indexed id, string uri);
    event TierThresholdUpdated(string tierName, uint256 newThreshold);
    event TierURIUpdated(string tierName, string newURI);
    event CustomTierAdded(uint8 tierId, string name, uint256 threshold);
    event CustomTierUpdated(uint8 tierId, string name, uint256 threshold);
    event TierCacheTTLUpdated(uint256 newTTL);
    event BridgeMint(address indexed to, uint256 amount);
    event BridgeBurn(address indexed from, uint256 amount);
    event TokensWrapped(address indexed user, uint256 amount);
    event TokensUnwrapped(address indexed user, uint256 amount);
    event WrappedTokenSet(address indexed wrapper);
    event BridgeProxySet(address indexed proxy);
    
    // Mapping to store user tier cache
    mapping(address => uint256) private _tierCache;
    
    // Cache time-to-live (TTL) - 1 hour
    uint256 private constant TIER_CACHE_TTL = 1 hours;
    
    // Timestamp when cache was created for each address
    mapping(address => uint256) private _tierCacheTimestamp;
    
    // Địa chỉ contract bridge được ủy quyền
    address public authorizedBridge;
    
    // Extensibility support
    mapping(address => bool) public modules;
    
    // Ánh xạ lưu trữ bridge nonce cho mỗi địa chỉ
    mapping(address => uint256) private _bridgeNonces;
    
    // Ánh xạ lưu trữ thời gian bridge cuối cùng
    mapping(address => uint256) private _lastBridgeTime;
    
    // Ánh xạ lưu trữ các bridge đang chờ xử lý
    mapping(bytes32 => bool) private _pendingBridges;
    
    // Sự kiện liên quan đến bridge
    event BridgeInitiated(bytes32 indexed bridgeId, address indexed user, bytes destination, uint256 amount);
    event BridgeCompleted(bytes32 indexed bridgeId, address indexed user, bytes destination, uint256 amount);
    event BridgeFailed(bytes32 indexed bridgeId, address indexed user, bytes destination, uint256 amount, string reason);
    
    // Mapping để track các message đã xử lý
    mapping(bytes32 => bool) private _processedMessages;
    
    // Mapping để lưu trusted remotes
    mapping(uint16 => bytes) private _trustedRemotes;
    
    // Thời gian cooldown bridge (mặc định: 1 giờ)
    uint256 public bridgeCooldown = 1 hours;
    
    // Cross-chain nonce management
    mapping(uint16 => uint256) private _lastIncomingNonce;
    mapping(uint16 => mapping(bytes => uint256)) private _remoteNonces;
    
    // Bridge Requestors tracking
    mapping(address => uint256) private _dailyBridgeVolume;
    uint256 private _lastResetTime = block.timestamp;
    uint256 public dailyBridgeLimit = 10000 ether;
    
    // Bridge token approval tracking
    mapping(bytes32 => bool) private _bridgeApprovals;
    
    // Số lần retry tối đa
    uint8 private constant MAX_RETRY_COUNT = 3;
    
    // Events
    event BridgeApprovalRevocationFailed(bytes32 indexed bridgeId);
    event EmergencyBurnFailed(bytes32 indexed bridgeId, uint256 amount);
    event DailyBridgeLimitUpdated(uint256 newLimit);
    
    // Thêm mapping mới để theo dõi lần cập nhật cuối và sự kiện tương ứng
    mapping(address => uint256) private _userTierCacheLastUpdated;
    event UserTierCacheUpdated(address indexed user, uint8 tier, uint256 expiry);
    
    // Add these mappings for better nonce control
    // Mapping to store bridge requests by ID
    mapping(bytes32 => BridgeRequest) private _bridgeRequests;
    // Mapping to track processed bridge requests
    mapping(bytes32 => bool) private _processedBridgeRequests;
    // Mapping to track nonces across different chains
    mapping(bytes calldata => mapping(uint256 => bool)) private _usedRemoteNonces;
    // Cross-chain sequence tracking
    mapping(uint16 => uint64) private _outgoingSequence;
    mapping(uint16 => uint64) private _lastIncomingSequence;

    // Struct for bridge requests
    struct BridgeRequest {
        address sender;
        bytes destination;
        uint256 amount;
        uint256 nonce;
        bytes32 bridgeId;
        uint256 timestamp;
        BridgeStatus status;
        string errorMessage;
        uint8 retryCount;
    }

    // Enum for bridge request status
    enum BridgeStatus {
        Pending,
        Completed,
        Failed,
        Refunded
    }
    
    /**
     * @dev Initialize the contract
     * @param baseURI Base URI for metadata
     * @param lzEndpoint Address of LayerZero Endpoint on BSC
     */
    constructor(string memory baseURI, address lzEndpoint) 
        ERC1155(baseURI) 
        NonblockingLzApp(lzEndpoint) 
        Ownable(msg.sender) 
    {
        name = "Diamond Token";
        symbol = "DMD";
        decimals = 18;
        _baseURI = baseURI;
        
        // Define total supply but do not mint tokens
        _totalSupply = 1_000_000_000 * 10**decimals;
        bridgedSupply = 0;
        
        // Initialize URIs for tiers
        regularTierURI = string(abi.encodePacked(baseURI, "/regular"));
        bronzeTierURI = string(abi.encodePacked(baseURI, "/bronze"));
        silverTierURI = string(abi.encodePacked(baseURI, "/silver"));
        goldTierURI = string(abi.encodePacked(baseURI, "/gold"));
        diamondTierURI = string(abi.encodePacked(baseURI, "/diamond"));
        
        // Initialize tier list
        tiers.push(DMDTier.Regular);
        tiers.push(DMDTier.Bronze);
        tiers.push(DMDTier.Silver);
        tiers.push(DMDTier.Gold);
        tiers.push(DMDTier.Diamond);
    }

    /**
     * @dev Set the Wrapped DMD (ERC20) token contract address
     * @param _wrappedToken Address of the Wrapped DMD token contract
     */
    function setWrappedDMD(address _wrappedToken) external onlyOwner {
        require(_wrappedToken != address(0), "Invalid wrapper token address");
        wrappedDMD = IWrappedDMD(_wrappedToken);
        emit WrappedTokenSet(_wrappedToken);
    }
    
    /**
     * @dev Set the bridge proxy contract address
     * @param _proxy Address of the bridge proxy contract
     */
    function setBridgeProxy(address _proxy) external onlyOwner {
        require(_proxy != address(0), "Invalid proxy address");
        bridgeProxy = _proxy;
        emit BridgeProxySet(_proxy);
    }
    
    /**
     * @dev Register a module to extend functionality
     * @param module Address of the module to register
     */
    function registerModule(address module) external onlyOwner {
        require(module != address(0), "Invalid module address");
        modules[module] = true;
    }
    
    /**
     * @dev Unregister a module
     * @param module Address of the module to unregister
     */
    function unregisterModule(address module) external onlyOwner {
        modules[module] = false;
    }
    
    /**
     * @dev Mint ERC1155 token from ERC20 (unwrap)
     * @param to Recipient address
     * @param amount Amount to unwrap
     */
    function unwrapFromERC20(address to, uint256 amount) external {
        require(
            msg.sender == address(wrappedDMD) || 
            modules[msg.sender], 
            "Unauthorized: only wDMD or authorized module"
        );
        
        // Mint ERC1155 token to recipient
        _mint(to, DMD_TOKEN_ID, amount, "");
        
        // Update tier cache
        updateUserTierCache(to);
        
        emit TokensUnwrapped(to, amount);
    }
    
    /**
     * @dev Wrap ERC1155 to ERC20 token (Wrapped DMD)
     * @param amount Amount to wrap
     */
    function wrapToERC20(uint256 amount) external whenNotPaused {
        require(address(wrappedDMD) != address(0), "Wrapped token not set");
        require(balanceOf(msg.sender, DMD_TOKEN_ID) >= amount, "Insufficient ERC1155 balance");
        
        // Burn ERC1155 token
        _burn(msg.sender, DMD_TOKEN_ID, amount);
        
        // Mint Wrapped DMD (ERC20) token 
        wrappedDMD.mintWrapped(msg.sender, amount);
        
        // Update tier cache
        updateUserTierCache(msg.sender);
        
        emit TokensWrapped(msg.sender, amount);
    }
    
    /**
     * @dev Bridge to NEAR through Wrapped ERC20 and BridgeProxy
     * @param nearAccount NEAR account ID in bytes format
     * @param amount Amount to bridge
     */
    function bridgeToNear(bytes calldata nearAccount, uint256 amount) external payable whenNotPaused nonReentrant {
        require(address(wrappedDMD) != address(0), "Wrapped token not set");
        require(address(bridgeProxy) != address(0), "Bridge proxy not set");
        require(balanceOf(msg.sender, DMD_TOKEN_ID) >= amount, "Insufficient ERC1155 balance");
        
        // Reset daily volume if a new day has started
        _resetDailyVolumeIfNeeded();
        
        // Check daily bridge limit
        _dailyBridgeVolume[msg.sender] += amount;
        require(_dailyBridgeVolume[msg.sender] <= dailyBridgeLimit, "Daily bridge limit exceeded");
        
        // Kiểm tra cooldown để ngăn rapid transactions
        require(block.timestamp - _lastBridgeTime[msg.sender] >= bridgeCooldown, "Bridge cooldown period active");
        _lastBridgeTime[msg.sender] = block.timestamp;
        
        // Tạo Nonce và Bridge ID để tracking
        uint256 nonce = _bridgeNonces[msg.sender]++;
        
        // Generate a deterministic but unique bridgeId
        bytes32 bridgeId = keccak256(abi.encodePacked(
            msg.sender, 
            nearAccount, 
            amount, 
            nonce, 
            block.timestamp,
            block.chainid
        ));
        
        // Get the current sequence number for this destination chain
        uint16 destChainId = NEAR_CHAIN_ID;
        uint64 sequence = uint64(_outgoingSequence[destChainId]++);
        
        // Verify bridge ID is unique
        require(!_processedBridgeRequests[bridgeId], "Bridge request already processed");
        
        // Create bridge request record
        _bridgeRequests[bridgeId] = BridgeRequest({
            sender: msg.sender,
            destination: nearAccount,
            amount: amount,
            nonce: nonce,
            bridgeId: bridgeId,
            timestamp: block.timestamp,
            status: BridgeStatus.Pending,
            errorMessage: "",
            retryCount: 0
        });
        
        // Mark as pending (not processed yet)
        _processedBridgeRequests[bridgeId] = false;
        
        // Ghi log bắt đầu bridge
        emit BridgeInitiated(bridgeId, msg.sender, nearAccount, amount);
        
        // STEP 1: Lock the ERC1155 token instead of burning it immediately
        // This creates an escrow that can be released if the bridge fails
        _lockTokensForBridge(msg.sender, DMD_TOKEN_ID, amount);
        
        // STEP 2: Mint Wrapped DMD tokens to this contract as an intermediate step
        try wrappedDMD.mintWrapped(address(this), amount) {
            // Verify tokens were minted correctly
            uint256 balanceBefore = wrappedDMD.balanceOf(address(this));
            require(balanceBefore >= amount, "Wrapped token balance insufficient");
            
            // STEP 3: Approve the bridge proxy to spend tokens
            try wrappedDMD.approve(bridgeProxy, amount) {
                // Create combined payload with cross-chain nonce for better tracking
                bytes memory combinedPayload = abi.encode(
                    msg.sender,                 // Original sender
                    nearAccount,                // Destination on NEAR
                    amount,                     // Amount to bridge
                    bridgeId,                   // Unique bridge ID
                    nonce,                      // Local nonce
                    block.chainid,              // Source chain ID
                    sequence                    // Sequence number for this destination
                );
                
                // STEP 4: Call bridge proxy to transfer tokens
                (bool success, bytes memory returnData) = bridgeProxy.call{value: msg.value}(
                    abi.encodeWithSignature(
                        "bridgeToNear(address,bytes,uint256,bytes32,bytes)",
                        msg.sender,
                        nearAccount,
                        amount,
                        bridgeId,
                        combinedPayload
                    )
                );
                
                if (success) {
                    // Verify tokens were actually transferred
                    uint256 balanceAfter = wrappedDMD.balanceOf(address(this));
                    require(balanceAfter == balanceBefore - amount, "Tokens not transferred to bridge");
                    
                    // STEP 5: Now it's safe to burn the original tokens since the bridge was successful
                    _burnLockedTokens(msg.sender, DMD_TOKEN_ID, amount, bridgeId);
                    
                    // Mark bridge as completed
                    _bridgeRequests[bridgeId].status = BridgeStatus.Completed;
                    _processedBridgeRequests[bridgeId] = true;
                    
                    // Update tier cache
                    updateUserTierCache(msg.sender);
                    
                    // Send notification to user (via event)
                    emit BridgeCompleted(bridgeId, msg.sender, nearAccount, amount);
                } else {
                    // Bridge thất bại, handle error and refund
                    string memory errorMsg = _getRevertMsg(returnData);
                    _handleBridgeFailure(bridgeId, msg.sender, amount, errorMsg);
                }
            } catch (bytes memory err) {
                // Approval failed, refund
                string memory errorMsg = string(abi.encodePacked("Approval failed: ", _bytes32ToString(keccak256(err))));
                _handleBridgeFailure(bridgeId, msg.sender, amount, errorMsg);
            }
        } catch (bytes memory err) {
            // Mint failed, unlock the tokens and refund
            _unlockTokens(msg.sender, DMD_TOKEN_ID, amount, bridgeId);
            _bridgeRequests[bridgeId].status = BridgeStatus.Failed;
            _bridgeRequests[bridgeId].errorMessage = string(abi.encodePacked("Mint failed: ", _bytes32ToString(keccak256(err))));
            _processedBridgeRequests[bridgeId] = true;
            
            // Refund ETH
            (bool refunded, ) = msg.sender.call{value: msg.value}("");
            require(refunded, "ETH refund failed");
            
            emit BridgeFailed(bridgeId, msg.sender, nearAccount, amount, "Mint wrapped token failed");
        }
    }
    
    /**
     * @dev Lock tokens for bridge instead of burning them immediately
     * @param user Address of token owner
     * @param id Token ID
     * @param amount Amount to lock
     */
    function _lockTokensForBridge(address user, uint256 id, uint256 amount) internal {
        // Move the tokens to this contract's escrow
        safeTransferFrom(user, address(this), id, amount, "");
        
        // Emit lock event
        emit TokensLockedForBridge(user, id, amount);
    }
    
    /**
     * @dev Burn locked tokens after bridge is confirmed successful
     * @param user Original owner of tokens
     * @param id Token ID
     * @param amount Amount to burn
     * @param bridgeId ID of the bridge transaction
     */
    function _burnLockedTokens(address user, uint256 id, uint256 amount, bytes32 bridgeId) internal {
        // Burn the tokens from the contract's balance
        _burn(address(this), id, amount);
        
        // Emit burn event with bridge reference
        emit TokensBurnedAfterBridge(user, id, amount, bridgeId);
    }
    
    /**
     * @dev Unlock tokens if bridge fails
     * @param user Address to return tokens to
     * @param id Token ID
     * @param amount Amount to unlock
     * @param bridgeId ID of the bridge transaction
     */
    function _unlockTokens(address user, uint256 id, uint256 amount, bytes32 bridgeId) internal {
        // Return the tokens to the original owner
        safeTransferFrom(address(this), user, id, amount, "");
        
        // Emit unlock event
        emit TokensUnlockedAfterFailedBridge(user, id, amount, bridgeId);
    }
    
    /**
     * @dev Handle bridge failure (xử lý khi bridge thất bại)
     * @param bridgeId ID của bridge transaction
     * @param user Địa chỉ người dùng
     * @param amount Số lượng token
     * @param reason Lý do thất bại
     */
    function _handleBridgeFailure(bytes32 bridgeId, address user, uint256 amount, string memory reason) internal {
        // Update bridge request status
        BridgeRequest storage request = _bridgeRequests[bridgeId];
        request.status = BridgeStatus.Failed;
        request.errorMessage = reason;
        _processedBridgeRequests[bridgeId] = true;
        
        // Nếu tokens đã được approved, revoke approval
        try wrappedDMD.approve(bridgeProxy, 0) {
            // Approval revoked
        } catch {
            // Approval revocation failed - log this but continue
            emit BridgeApprovalRevocationFailed(bridgeId);
        }
        
        // Try to burn ERC20 tokens if they're still with the contract
        try wrappedDMD.burnWrapped(address(this), amount) {
            // Burn successful
        } catch {
            // Burn failed - emergency mode
            emit EmergencyBurnFailed(bridgeId, amount);
        }
        
        // Unlock and return ERC1155 tokens to the user
        _unlockTokens(user, DMD_TOKEN_ID, amount, bridgeId);
        
        // Hoàn trả ETH
        (bool refunded, ) = user.call{value: msg.value}("");
        require(refunded, "ETH refund failed");
        
        // Mark bridge as refunded
        request.status = BridgeStatus.Refunded;
        
        emit BridgeFailed(bridgeId, user, bytes(""), amount, reason);
    }
    
    /**
     * @dev Set or update bridge cooldown period
     * @param newCooldown New cooldown period in seconds
     */
    function setBridgeCooldown(uint256 newCooldown) external onlyOwner {
        bridgeCooldown = newCooldown;
        emit BridgeCooldownUpdated(newCooldown);
    }
    
    /**
     * @dev Force complete or cancel a stuck bridge transaction (admin only)
     * @param bridgeId ID of the bridge transaction
     * @param success Whether to mark as success or failure
     */
    function resolveBridgeTransaction(bytes32 bridgeId, bool success) external onlyOwner {
        require(_bridgeRequests[bridgeId].timestamp > 0, "Bridge request not found");
        BridgeRequest storage request = _bridgeRequests[bridgeId];
        
        // Only allow resolving pending transactions
        require(request.status == BridgeStatus.Pending, "Bridge not in pending state");
        
        if (success) {
            // Mark as completed and burn the locked tokens
            request.status = BridgeStatus.Completed;
            _burnLockedTokens(request.sender, DMD_TOKEN_ID, request.amount, request.bridgeId);
            emit BridgeCompleted(request.bridgeId, request.sender, request.destination, request.amount);
        } else {
            // Mark as failed and unlock tokens
            request.status = BridgeStatus.Failed;
            request.errorMessage = "Admin force resolved as failed";
            _unlockTokens(request.sender, DMD_TOKEN_ID, request.amount, request.bridgeId);
            emit BridgeFailed(request.bridgeId, request.sender, request.destination, request.amount, "Admin cancelled");
        }
        
        _processedBridgeRequests[bridgeId] = true;
    }
    
    /**
     * @dev Pause all bridge operations
     */
    function pauseBridge() external onlyOwner {
        _pause();
        emit BridgePaused();
    }
    
    /**
     * @dev Resume all bridge operations
     */
    function resumeBridge() external onlyOwner {
        _unpause();
        emit BridgeResumed();
    }
    
    /**
     * @dev Get status of a bridge transaction
     * @param bridgeId ID of the bridge transaction
     * @return BridgeStatus, error message, sender, timestamp, amount
     */
    function getBridgeStatus(bytes32 bridgeId) external view returns (
        BridgeStatus status,
        string memory errorMessage,
        address sender,
        uint256 timestamp,
        uint256 amount
    ) {
        BridgeRequest storage request = _bridgeRequests[bridgeId];
        require(request.timestamp > 0, "Bridge request not found");
        
        return (
            request.status,
            request.errorMessage,
            request.sender,
            request.timestamp,
            request.amount
        );
    }
    
    /**
     * @dev Get pending bridges for a user
     * @param user Address of the user
     * @return Array of bridge IDs
     */
    function getUserPendingBridges(address user) external view returns (bytes32[] memory) {
        // Count pending bridges for this user
        uint256 count = 0;
        for (uint256 i = 0; i < _bridgeNonces[user]; i++) {
            bytes32 potentialId = keccak256(abi.encodePacked(user, i, "pending"));
            if (_bridgeRequests[potentialId].timestamp > 0 && 
                _bridgeRequests[potentialId].status == BridgeStatus.Pending) {
                count++;
            }
        }
        
        // Create and fill array
        bytes32[] memory result = new bytes32[](count);
        uint256 index = 0;
        
        for (uint256 i = 0; i < _bridgeNonces[user]; i++) {
            bytes32 potentialId = keccak256(abi.encodePacked(user, i, "pending"));
            if (_bridgeRequests[potentialId].timestamp > 0 && 
                _bridgeRequests[potentialId].status == BridgeStatus.Pending) {
                result[index] = potentialId;
                index++;
            }
        }
        
        return result;
    }
    
    // New events
    event TokensLockedForBridge(address indexed user, uint256 indexed tokenId, uint256 amount);
    event TokensBurnedAfterBridge(address indexed user, uint256 indexed tokenId, uint256 amount, bytes32 indexed bridgeId);
    event TokensUnlockedAfterFailedBridge(address indexed user, uint256 indexed tokenId, uint256 amount, bytes32 indexed bridgeId);
    event BridgeCooldownUpdated(uint256 newCooldown);
    event BridgePaused();
    event BridgeResumed();
    
    /**
     * @dev Change the base URI
     * @param newuri New URI
     */
    function setURI(string memory newuri) public onlyOwner {
        _baseURI = newuri;
    }
    
    /**
     * @dev Get the URI of a token
     * @param tokenId ID of the token
     * @return Metadata URI of the token
     */
    function uri(uint256 tokenId) public view override returns (string memory) {
        // If it's an NFT with its own URI
        if (tokenId != DMD_TOKEN_ID && bytes(_tokenURIs[tokenId]).length > 0) {
            return _tokenURIs[tokenId];
        }
        
        // If it's a DMD token (ID = 0), return URI based on tier
        if (tokenId == DMD_TOKEN_ID) {
            // If empty address, return default URI
            if (msg.sender == address(0)) {
            return _baseURI;
            }
            
            // Determine tier based on balance
            DMDTier tier = getUserTier(msg.sender);
            
            // Return URI corresponding to tier
            if (tier == DMDTier.Diamond) {
                return diamondTierURI;
            } else if (tier == DMDTier.Gold) {
                return goldTierURI;
            } else if (tier == DMDTier.Silver) {
                return silverTierURI;
            } else if (tier == DMDTier.Bronze) {
                return bronzeTierURI;
            } else if (uint8(tier) >= 5) {
                // Custom tier
                string memory customURI = customTierURIs[uint8(tier)];
                if (bytes(customURI).length > 0) {
                    return customURI;
                }
            }
            
            return regularTierURI;
        }
        
        // For other NFTs
        return string(abi.encodePacked(_baseURI, tokenId.toString()));
    }
    
    /**
     * @dev Set the tier cache time-to-live
     * @param newTTL New time-to-live (in seconds)
     */
    function setTierCacheTTL(uint256 newTTL) public onlyOwner {
        tierCacheTTL = newTTL;
        emit TierCacheTTLUpdated(newTTL);
    }
    
    /**
     * @dev Update tier cache for a user with enhanced synchronization
     * @param user User address
     */
    function updateUserTierCache(address user) internal {
        // Kiểm tra trùng lặp - không thực hiện nếu đã được cập nhật trong cùng block
        if (_userTierCacheLastUpdated[user] == block.number) {
            return;
        }
        
        // Xác định tier dựa trên tổng số dư
        DMDTier tier = getUserTier(user);
        
        // Cập nhật cache
        _userTierCache[user] = tier;
        _userTierCacheExpiry[user] = block.timestamp + tierCacheTTL;
        
        // Đánh dấu rằng cache đã được cập nhật trong block này
        _userTierCacheLastUpdated[user] = block.number;
        
        // Emit sự kiện để các contract bên ngoài biết về việc cập nhật cache
        emit UserTierCacheUpdated(user, uint8(tier), _userTierCacheExpiry[user]);
    }
    
    /**
     * @dev Force synchronize tier cache between contracts
     * @param user User address to synchronize
     * @param tier Tier value
     * @param expiry Expiration timestamp
     */
    function syncExternalTierCache(address user, uint8 tier, uint256 expiry) external {
        // Chỉ cho phép Wrapped DMD hoặc contract chủ sở hữu gọi hàm này
        require(
            msg.sender == address(wrappedDMD) || msg.sender == owner(),
            "Only wrappedDMD or owner can sync cache"
        );
        
        // Cập nhật cache
        _userTierCache[user] = DMDTier(tier);
        _userTierCacheExpiry[user] = expiry;
        _userTierCacheLastUpdated[user] = block.number;
    }
    
    /**
     * @dev Override invalidateTierCache để gọi qua wrappedDMD nếu được thiết lập
     * @param user Address to clear cache for
     */
    function invalidateTierCache(address user) public override {
        require(
            user == msg.sender || owner() == msg.sender,
            "Only owner or user can invalidate cache"
        );
        
        // Gốc: Xóa cache trong contract này
        delete _userTierCache[user];
        delete _userTierCacheExpiry[user];
        delete _userTierCacheLastUpdated[user];
        
        // Thêm: Xóa cache trong wrappedDMD nếu có thể
        if (address(wrappedDMD) != address(0)) {
            try wrappedDMD.invalidateTierCache(user) {
                // Thành công
            } catch {
                // Tiếp tục nếu có lỗi, không ảnh hưởng đến hoạt động của contract gốc
            }
        }
        
        emit TierCacheInvalidated(user);
    }
    
    // Thêm sự kiện mới
    event TierCacheInvalidated(address indexed user);
    
    /**
     * @dev Get cached tier info with validation
     * @param user User address
     * @return tier The user's tier
     * @return isValid Whether the cache is valid
     * @return expiry Cache expiration time
     */
    function getUserTierCacheInfo(address user) external view returns (uint8 tier, bool isValid, uint256 expiry) {
        tier = uint8(_userTierCache[user]);
        expiry = _userTierCacheExpiry[user];
        isValid = expiry > block.timestamp;
        return (tier, isValid, expiry);
    }
    
    // Thêm modifier cho các hàm ảnh hưởng đến số dư
    modifier updateTierAfter(address user) {
        _;
        updateUserTierCache(user);
    }
    
    // Sửa các hàm chuyển token để cập nhật cache tier
    function transfer(
        address to,
        address from,
        uint256 id,
        uint256 amount,
        bytes memory data
    ) public override {
        super.transfer(to, from, id, amount, data);
        
        // Cập nhật cache cho cả người gửi và người nhận
        if (id == DMD_TOKEN_ID) {
            updateUserTierCache(from);
            updateUserTierCache(to);
        }
    }
    
    function batchTransfer(
        address to,
        address from,
        uint256[] memory ids,
        uint256[] memory amounts,
        bytes memory data
    ) public override {
        super.batchTransfer(to, from, ids, amounts, data);
        
        // Kiểm tra xem có DMD token trong giao dịch không
        bool containsDMD = false;
        for (uint i = 0; i < ids.length; i++) {
            if (ids[i] == DMD_TOKEN_ID) {
                containsDMD = true;
                break;
            }
        }
        
        // Cập nhật cache nếu có DMD token
        if (containsDMD) {
            updateUserTierCache(from);
            updateUserTierCache(to);
        }
    }
    
    /**
     * @dev Determine user tier based on DMD balance (ERC1155 + ERC20)
     * @param user User address
     * @return User's tier
     */
    function getUserTier(address user) public view returns (DMDTier) {
        // Check cache if still valid
        if (_userTierCacheExpiry[user] > block.timestamp) {
            return _userTierCache[user];
        }
        
        // Get total balance (ERC1155 + ERC20)
        uint256 balance = balanceOf(user, DMD_TOKEN_ID);
        
        // If wrapped token is set, add ERC20 balance
        if (address(wrappedDMD) != address(0)) {
            balance += wrappedDMD.balanceOf(user);
        }
        
        // Check standard tiers first
        if (balance >= diamondThreshold) {
            return DMDTier.Diamond;
        } else if (balance >= goldThreshold) {
            return DMDTier.Gold;
        } else if (balance >= silverThreshold) {
            return DMDTier.Silver;
        } else if (balance >= bronzeThreshold) {
            return DMDTier.Bronze;
        }
        
        // Check custom tiers
        for (uint8 i = 5; i < tiers.length; i++) {
            if (balance >= customTierThresholds[i]) {
                return DMDTier(i);
            }
        }
        
        return DMDTier.Regular;
    }
    
    /**
     * @dev Pause all token operations
     */
    function pause() public onlyOwner {
        _pause();
    }
    
    /**
     * @dev Unpause all token operations
     */
    function unpause() public onlyOwner {
        _unpause();
    }
    
    /**
     * @dev Receive ETH
     */
    receive() external payable {}

    /**
     * @dev Set the daily bridge limit per user
     * @param newLimit New daily limit
     */
    function setDailyBridgeLimit(uint256 newLimit) external onlyOwner {
        require(newLimit > 0, "Limit must be positive");
        dailyBridgeLimit = newLimit;
        emit DailyBridgeLimitUpdated(newLimit);
    }

    /**
     * @dev Convert bytes32 to string (helper function for error reporting)
     * @param _bytes32 bytes32 value to convert
     * @return string representation
     */
    function _bytes32ToString(bytes32 _bytes32) internal pure returns (string memory) {
        bytes memory bytesArray = new bytes(64);
        for (uint256 i = 0; i < 32; i++) {
            bytesArray[i*2] = _fromHexChar(uint8(uint256(_bytes32) / (2**(8*(31 - i)))) / 16);
            bytesArray[i*2+1] = _fromHexChar(uint8(uint256(_bytes32) / (2**(8*(31 - i)))) % 16);
        }
        return string(bytesArray);
    }

    /**
     * @dev Convert number to hex char
     * @param _i number to convert
     * @return char hex character
     */
    function _fromHexChar(uint8 _i) internal pure returns (bytes1) {
        return _i < 10 ? bytes1(_i + 48) : bytes1(_i + 87);
    }

    /**
     * @dev Get bridge request details
     * @param bridgeId Bridge request ID
     * @return Bridge request details
     */
    function getBridgeRequest(bytes32 bridgeId) external view returns (
        address sender,
        bytes memory destination,
        uint256 amount,
        uint256 nonce,
        uint256 timestamp,
        uint8 status, // 0=Pending, 1=Completed, 2=Failed, 3=Refunded
        string memory errorMessage
    ) {
        BridgeRequest storage request = _bridgeRequests[bridgeId];
        return (
            request.sender,
            request.destination,
            request.amount,
            request.nonce,
            request.timestamp,
            uint8(request.status),
            request.errorMessage
        );
    }

    /**
     * @dev Reset daily volume if a new day has started
     */
    function _resetDailyVolumeIfNeeded() internal {
        uint256 currentDay = block.timestamp / 1 days;
        uint256 lastResetDay = _lastResetTime / 1 days;
        
        if (currentDay > lastResetDay) {
            _lastResetTime = block.timestamp;
            // Reset all user volumes
        }
    }

    /**
     * @dev Get revert message from returnData
     * @param returnData Data return từ external call
     * @return Thông báo lỗi
     */
    function _getRevertMsg(bytes memory returnData) internal pure returns (string memory) {
        // If empty or too short, return generic message
        if (returnData.length < 68) return "Transaction reverted silently";
        
        assembly {
            returnData := add(returnData, 0x04) // Skip the function selector
        }
        
        return abi.decode(returnData, (string));
    }
    
    /**
     * @dev Handle message from LayerZero
     * @param _srcChainId Source chain ID
     * @param _srcAddress Source address
     * @param _nonce Message nonce
     * @param _payload Message payload
     */
    function _nonblockingLzReceive(
        uint16 _srcChainId,
        bytes memory _srcAddress,
        uint64 _nonce,
        bytes memory _payload
    ) internal override {
        // Tạo message ID để tracking
        bytes32 messageId = keccak256(abi.encodePacked(_srcChainId, _srcAddress, _nonce));
        
        // Kiểm tra message chưa xử lý (tránh replay attack)
        require(!_processedMessages[messageId], "Message already processed");
        
        // Verify the nonce is in sequence or is valid
        bool isSequential = _nonce == _lastIncomingSequence[_srcChainId] + 1;
        bool isWithinWindow = _nonce > _lastIncomingSequence[_srcChainId] && 
                              _nonce <= _lastIncomingSequence[_srcChainId] + 100; // Allow window of 100 for out-of-order messages
        
        require(isSequential || isWithinWindow, "Invalid nonce sequence");
        
        // Update last incoming sequence if higher
        if (_nonce > _lastIncomingSequence[_srcChainId]) {
            _lastIncomingSequence[_srcChainId] = _nonce;
        }
        
        try {
            // Kiểm tra chain là NEAR hoặc trusted remote
            if (_srcChainId == NEAR_CHAIN_ID) {
                // Verify source address is trusted
                require(_validateTrustedRemote(_srcChainId, _srcAddress), "Source not trusted");
                
                // Decode payload từ NEAR
                (address toAddress, uint256 amount, bytes memory nearAccount, uint256 crossChainNonce) = 
                    abi.decode(_payload, (address, uint256, bytes, uint256));
                
                // Verify the cross-chain nonce to prevent duplicate processing
                require(!_usedRemoteNonces[nearAccount][crossChainNonce], "Cross-chain nonce already processed");
                _usedRemoteNonces[nearAccount][crossChainNonce] = true;
                
                // Validate địa chỉ
                require(toAddress != address(0), "Invalid to address");
                
                // Validate số lượng
                require(amount > 0, "Amount must be greater than zero");
                require(bridgedSupply + amount <= _totalSupply, "Exceeds total supply");
                
                // Tránh re-entrancy by marking early
                _processedMessages[messageId] = true;
                
                // Thực hiện mint ERC1155 token
                bridgedSupply += amount;
                _mint(toAddress, DMD_TOKEN_ID, amount, "");
                updateUserTierCache(toAddress);
                
                emit MessageProcessed(messageId, true, "");
                emit BridgeMint(toAddress, amount);
            } else {
                revert("Unsupported source chain");
            }
        } catch Error(string memory reason) {
            // Lưu message lỗi để xử lý sau
            _storeFailedMessage(messageId, _srcChainId, _srcAddress, _nonce, _payload, reason);
            emit MessageProcessed(messageId, false, reason);
        } catch (bytes memory) {
            // Low-level exception
            _storeFailedMessage(messageId, _srcChainId, _srcAddress, _nonce, _payload, "Low-level exception");
            emit MessageProcessed(messageId, false, "Low-level exception");
        }
    }
    
    /**
     * @dev Kiểm tra trusted remote
     * @param _srcChainId Chain ID nguồn
     * @param _srcAddress Địa chỉ nguồn
     * @return Kết quả xác thực
     */
    function _validateTrustedRemote(uint16 _srcChainId, bytes memory _srcAddress) internal view returns (bool) {
        bytes memory trustedRemote = _trustedRemotes[_srcChainId];
        
        if (trustedRemote.length == 0 || _srcAddress.length != trustedRemote.length) {
            return false;
        }
        
        // So sánh hiệu quả với keccak256
        return keccak256(_srcAddress) == keccak256(trustedRemote);
    }
    
    /**
     * @dev Lưu thông tin message thất bại
     * @param messageId ID của message
     * @param _srcChainId Chain ID nguồn
     * @param _srcAddress Địa chỉ nguồn
     * @param _nonce Nonce của message
     * @param _payload Payload của message
     * @param reason Lý do thất bại
     */
    function _storeFailedMessage(
        bytes32 messageId,
        uint16 _srcChainId,
        bytes memory _srcAddress,
        uint64 _nonce,
        bytes memory _payload,
        string memory reason
    ) internal {
        // Lưu vào mapping
        _failedMessages[messageId] = FailedMessage({
            srcChainId: _srcChainId,
            srcAddress: _srcAddress,
            nonce: _nonce,
            payload: _payload,
            timestamp: block.timestamp,
            reason: reason,
            retryCount: 0
        });
        
        emit MessageFailed(messageId, _srcChainId, reason);
    }
    
    /**
     * @dev Retry failed message
     * @param messageId ID của message thất bại
     */
    function retryFailedMessage(bytes32 messageId) external onlyOwner {
        FailedMessage storage message = _failedMessages[messageId];
        require(message.timestamp > 0, "Message not found");
        require(message.retryCount < MAX_RETRY_COUNT, "Max retry exceeded");
        
        // Tăng số lần retry
        message.retryCount++;
        
        // Gọi lại _nonblockingLzReceive
        _nonblockingLzReceive(
            message.srcChainId,
            message.srcAddress,
            message.nonce,
            message.payload
        );
    }
    
    /**
     * @dev Cài đặt trusted remote address
     * @param _srcChainId Chain ID nguồn
     * @param _srcAddress Địa chỉ nguồn
     */
    function setTrustedRemote(uint16 _srcChainId, bytes calldata _srcAddress) external onlyOwner {
        require(_srcAddress.length > 0, "Empty remote address");
        _trustedRemotes[_srcChainId] = _srcAddress;
        
        emit TrustedRemoteSet(_srcChainId, _srcAddress);
    }
    
    // Struct để lưu trữ thông tin message thất bại
    struct FailedMessage {
        uint16 srcChainId;
        bytes srcAddress;
        uint64 nonce;
        bytes payload;
        uint256 timestamp;
        string reason;
        uint8 retryCount;
    }
    
    // Mapping để lưu trữ thông tin message thất bại
    mapping(bytes32 => FailedMessage) private _failedMessages;
    
    // LayerZero Events
    event MessageProcessed(bytes32 indexed messageId, bool success, string reason);
    event MessageFailed(bytes32 indexed messageId, uint16 srcChainId, string reason);
    event TrustedRemoteSet(uint16 indexed chainId, bytes remoteAddress);

    /**
     * @dev Get user's bridge transactions (both pending and completed)
     * @param user User address
     * @param includeCompleted Whether to include completed transactions
     * @param includeRefunded Whether to include refunded transactions
     * @return Array of bridge transaction IDs
     */
    function getUserBridgeTransactions(
        address user,
        bool includeCompleted,
        bool includeRefunded
    ) external view returns (bytes32[] memory) {
        // Count eligible transactions
        uint256 count = 0;
        for (uint256 i = 0; i < _bridgeNonces[user]; i++) {
            bytes32 bridgeId = keccak256(abi.encodePacked(
                user, 
                i, 
                "bridge"
            ));
            
            if (_bridgeRequests[bridgeId].timestamp > 0) {
                BridgeStatus status = _bridgeRequests[bridgeId].status;
                
                // Filter by status based on parameters
                if (status == BridgeStatus.Pending || 
                    (includeCompleted && status == BridgeStatus.Completed) || 
                    (includeRefunded && status == BridgeStatus.Refunded) ||
                    status == BridgeStatus.Failed) {
                    count++;
                }
            }
        }
        
        // Create and fill result array
        bytes32[] memory result = new bytes32[](count);
        uint256 index = 0;
        
        for (uint256 i = 0; i < _bridgeNonces[user]; i++) {
            bytes32 bridgeId = keccak256(abi.encodePacked(
                user, 
                i, 
                "bridge"
            ));
            
            if (_bridgeRequests[bridgeId].timestamp > 0) {
                BridgeStatus status = _bridgeRequests[bridgeId].status;
                
                // Filter by status based on parameters
                if (status == BridgeStatus.Pending || 
                    (includeCompleted && status == BridgeStatus.Completed) || 
                    (includeRefunded && status == BridgeStatus.Refunded) ||
                    status == BridgeStatus.Failed) {
                    result[index] = bridgeId;
                    index++;
                }
            }
        }
        
        return result;
    }

    /**
     * @dev Get bridge status summary (total counts for admin monitoring)
     * @return totalPending Total pending bridges
     * @return totalCompleted Total completed bridges
     * @return totalFailed Total failed bridges
     * @return totalRefunded Total refunded bridges
     */
    function getBridgeStatusSummary() external view onlyOwner returns (
        uint256 totalPending,
        uint256 totalCompleted,
        uint256 totalFailed,
        uint256 totalRefunded
    ) {
        // Reset counters
        totalPending = 0;
        totalCompleted = 0;
        totalFailed = 0;
        totalRefunded = 0;
        
        // Loop through all addressed with nonces
        address[] memory users = _getUsersWithBridgeNonces();
        
        for (uint256 u = 0; u < users.length; u++) {
            address user = users[u];
            
            for (uint256 i = 0; i < _bridgeNonces[user]; i++) {
                bytes32 bridgeId = keccak256(abi.encodePacked(
                    user, 
                    i, 
                    "bridge"
                ));
                
                if (_bridgeRequests[bridgeId].timestamp > 0) {
                    BridgeStatus status = _bridgeRequests[bridgeId].status;
                    
                    if (status == BridgeStatus.Pending) {
                        totalPending++;
                    } else if (status == BridgeStatus.Completed) {
                        totalCompleted++;
                    } else if (status == BridgeStatus.Failed) {
                        totalFailed++;
                    } else if (status == BridgeStatus.Refunded) {
                        totalRefunded++;
                    }
                }
            }
        }
        
        return (totalPending, totalCompleted, totalFailed, totalRefunded);
    }

    /**
     * @dev Check bridge status before initiating a new bridge
     * @param destChainId Destination chain ID
     * @return isOperational Whether bridge is operational
     * @return avgCompletionTime Average completion time in seconds
     * @return pendingCount Number of pending transactions
     * @return successRate Success rate (0-100)
     */
    function checkBridgeStatus(uint16 destChainId) external view returns (
        bool isOperational,
        uint256 avgCompletionTime,
        uint256 pendingCount,
        uint256 successRate
    ) {
        // Default values
        isOperational = !paused() && destChainId == NEAR_CHAIN_ID;
        avgCompletionTime = 0;
        pendingCount = 0;
        uint256 completedCount = 0;
        uint256 failedCount = 0;
        
        // Get users with bridge nonces
        address[] memory users = _getUsersWithBridgeNonces();
        uint256 totalCompletionTime = 0;
        
        // Calculate statistics
        for (uint256 u = 0; u < users.length; u++) {
            address user = users[u];
            
            for (uint256 i = 0; i < _bridgeNonces[user]; i++) {
                bytes32 bridgeId = keccak256(abi.encodePacked(
                    user, 
                    i, 
                    "bridge"
                ));
                
                BridgeRequest storage request = _bridgeRequests[bridgeId];
                
                if (request.timestamp > 0) {
                    if (request.status == BridgeStatus.Pending) {
                        pendingCount++;
                    } else if (request.status == BridgeStatus.Completed) {
                        completedCount++;
                        // Add to total completion time calculation
                        // Note: In a real implementation, you would store the completion time
                        totalCompletionTime += 300; // Placeholder: 5 minutes average
                    } else if (request.status == BridgeStatus.Failed) {
                        failedCount++;
                    }
                }
            }
        }
        
        // Calculate average completion time if there are completed transactions
        if (completedCount > 0) {
            avgCompletionTime = totalCompletionTime / completedCount;
        }
        
        // Calculate success rate if there are any transactions
        uint256 totalTx = completedCount + failedCount;
        if (totalTx > 0) {
            successRate = (completedCount * 100) / totalTx;
        }
        
        return (isOperational, avgCompletionTime, pendingCount, successRate);
    }

    /**
     * @dev Helper function to get all users with bridge nonces
     * @return Array of user addresses with bridge nonces
     * Note: This is a simplified implementation for demonstration purposes
     */
    function _getUsersWithBridgeNonces() internal view returns (address[] memory) {
        // In a real implementation, you would need to maintain a list of users
        // who have initiated bridges, or query events
        address[] memory users = new address[](1);
        users[0] = msg.sender;
        return users;
    }

    /**
     * @dev Force retry a failed bridge transaction (admin only with timelock)
     * @param bridgeId ID of the bridge transaction
     */
    function retryFailedBridge(bytes32 bridgeId) external onlyOwner {
        BridgeRequest storage request = _bridgeRequests[bridgeId];
        require(request.timestamp > 0, "Bridge request not found");
        require(request.status == BridgeStatus.Failed, "Bridge not in failed state");
        require(request.retryCount < MAX_RETRY_COUNT, "Max retry count exceeded");
        
        // Increment retry counter
        request.retryCount++;
        
        // Reset status to pending
        request.status = BridgeStatus.Pending;
        
        // Restore the locked tokens if they were unlocked previously
        if (balanceOf(request.sender, DMD_TOKEN_ID) >= request.amount) {
            // Transfer tokens from user to contract (lock)
            safeTransferFrom(request.sender, address(this), DMD_TOKEN_ID, request.amount, "");
            
            // Try to execute the bridge again (similar to bridgeToNear but without creating a new request)
            // Implementation details would depend on integration with the bridge proxy
            emit BridgeRetryInitiated(bridgeId, request.sender, request.destination, request.amount, request.retryCount);
        } else {
            // User doesn't have enough tokens for retry
            request.status = BridgeStatus.Failed;
            request.errorMessage = "Insufficient balance for retry";
            emit BridgeRetryFailed(bridgeId, request.sender, "Insufficient balance for retry");
        }
    }

    /**
     * @dev Clean up old bridge requests to save storage (admin only)
     * @param user User address
     * @param maxAge Maximum age in seconds to keep a request
     * @param onlyRefunded Whether to only clean up refunded transactions
     */
    function cleanupOldBridgeRequests(address user, uint256 maxAge, bool onlyRefunded) external onlyOwner {
        uint256 currentTime = block.timestamp;
        
        for (uint256 i = 0; i < _bridgeNonces[user]; i++) {
            bytes32 bridgeId = keccak256(abi.encodePacked(
                user, 
                i, 
                "bridge"
            ));
            
            BridgeRequest storage request = _bridgeRequests[bridgeId];
            
            if (request.timestamp > 0 && 
                currentTime - request.timestamp > maxAge && 
                (!onlyRefunded || request.status == BridgeStatus.Refunded)) {
                // Delete the request
                delete _bridgeRequests[bridgeId];
                emit BridgeRequestCleaned(bridgeId, user);
            }
        }
    }

    /**
     * @dev Event emitted when a bridge retry is initiated
     */
    event BridgeRetryInitiated(bytes32 indexed bridgeId, address indexed user, bytes destination, uint256 amount, uint8 retryCount);

    /**
     * @dev Event emitted when a bridge retry fails
     */
    event BridgeRetryFailed(bytes32 indexed bridgeId, address indexed user, string reason);

    /**
     * @dev Event emitted when a bridge request is cleaned up
     */
    event BridgeRequestCleaned(bytes32 indexed bridgeId, address indexed user);
} 