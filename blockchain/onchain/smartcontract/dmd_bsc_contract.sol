// SPDX-License-Identifier: MIT
pragma solidity ^0.8.17;

import "@openzeppelin/contracts/token/ERC1155/ERC1155.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/security/Pausable.sol";
import "@openzeppelin/contracts/token/ERC1155/extensions/ERC1155Burnable.sol";
import "@openzeppelin/contracts/token/ERC1155/extensions/ERC1155Supply.sol";
import "@openzeppelin/contracts/utils/Strings.sol";
import "@layerzerolabs/solidity-examples/contracts/lzApp/NonblockingLzApp.sol";

// Interface cho ERC20DMD token
interface IERC20DMD {
    function mintWrapped(address to, uint256 amount) external;
    function burnWrapped(address from, uint256 amount) external;
    function balanceOf(address account) external view returns (uint256);
    function transfer(address to, uint256 amount) external returns (bool);
    function transferFrom(address from, address to, uint256 amount) external returns (bool);
}

// Interface cho BridgeERC20Proxy
interface IBridgeERC20Proxy {
    function bridgeToNear(address from, bytes calldata nearAddress, uint256 amount) external payable;
    function estimateBridgeFee(uint16 chainId) external view returns (uint256);
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
    
    // Địa chỉ của ERC20 Wrapper và Bridge Proxy
    IERC20DMD public erc20Wrapper;
    IBridgeERC20Proxy public bridgeProxy;
    
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
    event WrappedToERC20(address indexed user, uint256 amount);
    event UnwrappedFromERC20(address indexed user, uint256 amount);
    event ERC20WrapperSet(address indexed wrapper);
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
     * @dev Set the ERC20 wrapper contract address
     * @param _wrapper Address of the ERC20 wrapper contract
     */
    function setERC20Wrapper(address _wrapper) external onlyOwner {
        require(_wrapper != address(0), "Invalid wrapper address");
        erc20Wrapper = IERC20DMD(_wrapper);
        emit ERC20WrapperSet(_wrapper);
    }
    
    /**
     * @dev Set the bridge proxy contract address
     * @param _proxy Address of the bridge proxy contract
     */
    function setBridgeProxy(address _proxy) external onlyOwner {
        require(_proxy != address(0), "Invalid proxy address");
        bridgeProxy = IBridgeERC20Proxy(_proxy);
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
     * @dev Wrap ERC1155 token to ERC20 for bridging
     * @param amount Amount to wrap
     */
    function wrapToERC20(uint256 amount) external whenNotPaused {
        require(address(erc20Wrapper) != address(0), "ERC20 wrapper not set");
        require(balanceOf(msg.sender, DMD_TOKEN_ID) >= amount, "Insufficient balance");
        
        // Burn ERC1155 token
        _burn(msg.sender, DMD_TOKEN_ID, amount);
        
        // Mint ERC20 token via wrapper
        erc20Wrapper.mintWrapped(msg.sender, amount);
        
        emit WrappedToERC20(msg.sender, amount);
    }
    
    /**
     * @dev Unwrap ERC20 token back to ERC1155
     * @param to Recipient address
     * @param amount Amount to unwrap
     */
    function unwrapFromERC20(address to, uint256 amount) external {
        require(
            msg.sender == address(erc20Wrapper) || 
            msg.sender == address(bridgeProxy) || 
            modules[msg.sender], 
            "Unauthorized"
        );
        
        // Mint ERC1155 token to recipient
        _mint(to, DMD_TOKEN_ID, amount, "");
        
        // Update tier cache
        updateUserTierCache(to);
        
        emit UnwrappedFromERC20(to, amount);
    }
    
    /**
     * @dev Bridge ERC1155 token directly to NEAR (combines wrap and bridge)
     * @param nearAddress NEAR address in bytes format
     * @param amount Amount to bridge
     */
    function bridgeToNear(bytes calldata nearAddress, uint256 amount) external payable whenNotPaused {
        require(address(erc20Wrapper) != address(0), "ERC20 wrapper not set");
        require(address(bridgeProxy) != address(0), "Bridge proxy not set");
        require(balanceOf(msg.sender, DMD_TOKEN_ID) >= amount, "Insufficient balance");
        
        // First burn ERC1155 token
        _burn(msg.sender, DMD_TOKEN_ID, amount);
        
        // Then mint ERC20 token temporarily to the bridge proxy
        erc20Wrapper.mintWrapped(address(bridgeProxy), amount);
        
        // Finally bridge to NEAR through the proxy
        bridgeProxy.bridgeToNear{value: msg.value}(msg.sender, nearAddress, amount);
        
        emit BridgeBurn(msg.sender, amount);
    }
    
    /**
     * @dev Estimate fee for bridging to NEAR
     * @return Fee amount in native currency
     */
    function estimateBridgeFee() external view returns (uint256) {
        require(address(bridgeProxy) != address(0), "Bridge proxy not set");
        return bridgeProxy.estimateBridgeFee(NEAR_CHAIN_ID);
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
        (address toAddress, uint256 amount, string memory actionType) = abi.decode(_payload, (address, uint256, string));
        
        if (keccak256(bytes(actionType)) == keccak256(bytes("direct_erc1155"))) {
            // Mint directly as ERC1155
            bridgedSupply += amount;
            _mint(toAddress, DMD_TOKEN_ID, amount, "");
            updateUserTierCache(toAddress);
            emit BridgeMint(toAddress, amount);
        } else if (keccak256(bytes(actionType)) == keccak256(bytes("erc20"))) {
            // Mint as ERC20
            require(address(erc20Wrapper) != address(0), "ERC20 wrapper not set");
            erc20Wrapper.mintWrapped(toAddress, amount);
            bridgedSupply += amount;
        } else {
            // Default: mint as ERC1155
            bridgedSupply += amount;
            _mint(toAddress, DMD_TOKEN_ID, amount, "");
            updateUserTierCache(toAddress);
            emit BridgeMint(toAddress, amount);
        }
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
     * @dev Determine user tier based on DMD balance
     * @param user User address
     * @return User's tier
     */
    function getUserTier(address user) public view returns (DMDTier) {
        // Check cache if still valid
        if (_userTierCacheExpiry[user] > block.timestamp) {
            return _userTierCache[user];
        }
        
        uint256 balance = balanceOf(user, DMD_TOKEN_ID);
        
        // If user has wrapped to ERC20, also count that balance
        if (address(erc20Wrapper) != address(0)) {
            balance += erc20Wrapper.balanceOf(user);
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