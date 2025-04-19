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
    function bridgeToNear(bytes calldata nearAccount, uint256 amount) external payable whenNotPaused {
        require(address(wrappedDMD) != address(0), "Wrapped token not set");
        require(address(bridgeProxy) != address(0), "Bridge proxy not set");
        require(balanceOf(msg.sender, DMD_TOKEN_ID) >= amount, "Insufficient ERC1155 balance");
        
        // First convert ERC1155 to ERC20 (Wrapped DMD)
        _burn(msg.sender, DMD_TOKEN_ID, amount);
        wrappedDMD.mintWrapped(address(this), amount);
        
        // Then approve bridge proxy to use the Wrapped DMD
        wrappedDMD.burnWrapped(address(this), amount);
        
        // Forward the call to bridge proxy
        (bool success, ) = bridgeProxy.call{value: msg.value}(
            abi.encodeWithSignature(
                "bridgeToNear(address,bytes,uint256)",
                msg.sender,
                nearAccount,
                amount
            )
        );
        require(success, "Bridge call failed");
        
        // Update tier cache and emit event
        updateUserTierCache(msg.sender);
        emit BridgeBurn(msg.sender, amount);
    }
    
    /**
     * @dev Estimate fee for bridging to NEAR
     * Function will call the bridge proxy to get the fee estimate
     */
    function estimateBridgeFee(uint256 amount) external view returns (uint256) {
        require(address(bridgeProxy) != address(0), "Bridge proxy not set");
        
        // Call the bridge proxy method using low-level call
        (bool success, bytes memory data) = bridgeProxy.staticcall(
            abi.encodeWithSignature(
                "estimateBridgeFee(uint256)",
                amount
            )
        );
        
        require(success, "Fee estimation failed");
        return abi.decode(data, (uint256));
    }
    
    /**
     * @dev Get total balance (ERC1155 + ERC20) for an account
     * @param account Address to check
     * @return Total balance across both token formats
     */
    function getTotalBalance(address account) external view returns (uint256) {
        uint256 erc1155Balance = balanceOf(account, DMD_TOKEN_ID);
        uint256 erc20Balance = address(wrappedDMD) != address(0) ? wrappedDMD.balanceOf(account) : 0;
        
        return erc1155Balance + erc20Balance;
    }
    
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
     * @dev Clear the tier cache for an address
     * @param user Address to clear cache for
     */
    function invalidateTierCache(address user) public {
        require(
            user == msg.sender || owner() == msg.sender,
            "Only owner or user can invalidate cache"
        );
        delete _userTierCache[user];
        delete _userTierCacheExpiry[user];
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
     * @dev Update tier cache for a user
     * @param user User address
     */
    function updateUserTierCache(address user) internal {
        DMDTier tier = getUserTier(user);
        _userTierCache[user] = tier;
        _userTierCacheExpiry[user] = block.timestamp + tierCacheTTL;
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
     * @dev Bridge mint function - only called via LayerZero
     * @param to Address to mint tokens to
     * @param amount Amount to mint
     */
    function bridgeMint(address to, uint256 amount) external {
        require(_lzEndpoint.getChainId() == block.chainid, "DiamondToken: invalid endpoint");
        require(msg.sender == address(this), "DiamondToken: caller is not bridge adapter");
        require(bridgedSupply + amount <= _totalSupply, "DiamondToken: exceeds total supply");
        
        bridgedSupply += amount;
        _mint(to, DMD_TOKEN_ID, amount, "");
        
        // Update tier cache
        updateUserTierCache(to);
        
        emit BridgeMint(to, amount);
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
        require(_srcChainId == NEAR_CHAIN_ID, "Only accept messages from NEAR");
        
        // Decode payload from NEAR
        (address toAddress, uint256 amount, bytes memory nearAccount) = abi.decode(_payload, (address, uint256, bytes));
        
        // Bridge mint ERC1155 token by default
        bridgedSupply += amount;
        _mint(toAddress, DMD_TOKEN_ID, amount, "");
        updateUserTierCache(toAddress);
        
        emit BridgeMint(toAddress, amount);
    }
    
    /**
     * @dev Hook that is called before any token transfer
     */
    function _beforeTokenTransfer(
        address operator,
        address from,
        address to,
        uint256[] memory ids,
        uint256[] memory amounts,
        bytes memory data
    ) internal override(ERC1155, ERC1155Supply) whenNotPaused {
        super._beforeTokenTransfer(operator, from, to, ids, amounts, data);
        
        // If this is a mint or transfer (not a burn), update the recipient's tier cache
        if (to != address(0)) {
            updateUserTierCache(to);
        }
        
        // If this is a transfer (not a mint), update the sender's tier cache
            if (from != address(0)) {
            updateUserTierCache(from);
        }
    }
    
    /**
     * @dev Fallback function for extensibility
     * Allows registered modules to add functionality
     */
    fallback() external {
        address module = msg.sender;
        require(modules[module], "Module not registered");
        
        // Get function selector from calldata
        bytes4 selector = bytes4(msg.data[:4]);
        
        // Delegate call to module
        (bool success, ) = module.delegatecall(msg.data);
        require(success, "Module call failed");
    }
    
    /**
     * @dev Receive ETH
     */
    receive() external payable {}
} 