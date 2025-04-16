// SPDX-License-Identifier: MIT
pragma solidity ^0.8.17;

import "@openzeppelin/contracts/token/ERC1155/ERC1155.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/security/Pausable.sol";
import "@openzeppelin/contracts/token/ERC1155/extensions/ERC1155Burnable.sol";
import "@openzeppelin/contracts/token/ERC1155/extensions/ERC1155Supply.sol";
import "@openzeppelin/contracts/utils/Strings.sol";
import "@layerzerolabs/solidity-examples/contracts/lzApp/NonblockingLzApp.sol";

/**
 * @title DiamondToken
 * @dev DMD Token ERC-1155 standard on Base L2 with LayerZero bridging capability
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
    
    // LayerZero endpoint chain IDs
    uint16 public constant ETH_CHAIN_ID = 101;   // LayerZero chain ID for Ethereum
    uint16 public constant BSC_CHAIN_ID = 102;   // LayerZero chain ID for BSC
    uint16 public constant NEAR_CHAIN_ID = 115;  // LayerZero chain ID for NEAR
    uint16 public constant POLYGON_CHAIN_ID = 109; // LayerZero chain ID for Polygon
    uint16 public constant ARBITRUM_CHAIN_ID = 110; // LayerZero chain ID for Arbitrum
    
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
    event TokenBridged(address indexed from, uint256 indexed id, uint256 amount, uint16 toChainId, bytes toAddress);
    event TokenReceived(uint16 indexed fromChainId, bytes fromAddress, uint256 indexed id, uint256 amount, address to);
    event NFTMinted(address indexed to, uint256 indexed id, string uri);
    event TierThresholdUpdated(string tierName, uint256 newThreshold);
    event TierURIUpdated(string tierName, string newURI);
    event CustomTierAdded(uint8 tierId, string name, uint256 threshold);
    event CustomTierUpdated(uint8 tierId, string name, uint256 threshold);
    event TierCacheTTLUpdated(uint256 newTTL);
    
    // Mapping to store user tier cache
    mapping(address => uint256) private _tierCache;
    
    // Cache time-to-live (TTL) - 1 hour
    uint256 private constant TIER_CACHE_TTL = 1 hours;
    
    // Timestamp when cache was created for each address
    mapping(address => uint256) private _tierCacheTimestamp;
    
    /**
     * @dev Initialize the contract
     * @param baseURI Base URI for metadata
     * @param lzEndpoint Address of LayerZero Endpoint on Base L2
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
        
        // Initialize DMD total supply: 1 billion tokens (18 decimals)
        _totalSupply = 1_000_000_000 * 10**decimals;
        
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
        
        // Mint all tokens to contract creator
        _mint(msg.sender, DMD_TOKEN_ID, _totalSupply, "");
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
    
    // Bridge-related functions 
    
    /**
     * @dev Bridge token to Ethereum (convenience method)
     * @param toAddress Destination address on Ethereum
     * @param id Token ID to bridge
     * @param amount Token amount
     */
    function bridgeToETH(bytes memory toAddress, uint256 id, uint256 amount) public payable {
        bridgeToOtherChain(ETH_CHAIN_ID, toAddress, id, amount);
    }
    
    /**
     * @dev Bridge token to BSC (convenience method)
     * @param toAddress Destination address on BSC
     * @param id Token ID to bridge
     * @param amount Token amount
     */
    function bridgeToBSC(bytes memory toAddress, uint256 id, uint256 amount) public payable {
        bridgeToOtherChain(BSC_CHAIN_ID, toAddress, id, amount);
    }
    
    /**
     * @dev Bridge token to NEAR (convenience method)
     * @param toAddress Destination address on NEAR
     * @param id Token ID to bridge
     * @param amount Token amount
     */
    function bridgeToNEAR(bytes memory toAddress, uint256 id, uint256 amount) public payable {
        bridgeToOtherChain(NEAR_CHAIN_ID, toAddress, id, amount);
    }
    
    /**
     * @dev Bridge token to Polygon (convenience method)
     * @param toAddress Destination address on Polygon
     * @param id Token ID to bridge
     * @param amount Token amount
     */
    function bridgeToPolygon(bytes memory toAddress, uint256 id, uint256 amount) public payable {
        bridgeToOtherChain(POLYGON_CHAIN_ID, toAddress, id, amount);
    }
    
    /**
     * @dev Bridge token to Arbitrum (convenience method)
     * @param toAddress Destination address on Arbitrum
     * @param id Token ID to bridge
     * @param amount Token amount
     */
    function bridgeToArbitrum(bytes memory toAddress, uint256 id, uint256 amount) public payable {
        bridgeToOtherChain(ARBITRUM_CHAIN_ID, toAddress, id, amount);
    }
    
    /**
     * @dev Bridge token to another chain
     * @param toAddress Destination address on target chain
     * @param id Token ID to bridge
     * @param amount Token amount
     */
    function bridgeToOtherChain(uint16 chainId, bytes memory toAddress, uint256 id, uint256 amount) public payable {
        require(balanceOf(_msgSender(), id) >= amount, "Insufficient token balance");
        
        // Burn tokens before transferring
        if (id == DMD_TOKEN_ID) {
            _totalSupply -= amount;
            
            // Update tier cache
            invalidateTierCache(_msgSender());
        }
        _burn(_msgSender(), id, amount);
        
        // Construct payload to send through LayerZero
        bytes memory payload = abi.encode(id, amount, toAddress, _msgSender());
        
        // Send message through LayerZero
        _lzSend(
            chainId,                 // Destination chain ID in LayerZero
            payload,                 // Data to send
            payable(_msgSender()),   // refundAddress
            address(0x0),            // _zroPaymentAddress (0x0 = not using ZRO token for payment)
            bytes(""),               // _adapterParams
            msg.value                // Native fee for the relayer
        );
        
        emit TokenBridged(_msgSender(), id, amount, chainId, toAddress);
    }
    
    /**
     * @dev Receive function from LayerZero (from other chains)
     */
    function _nonblockingLzReceive(
        uint16 _srcChainId,
        bytes memory _srcAddress,
        uint64 _nonce,
        bytes memory _payload
    ) internal override {
        // Decode payload
        (
            uint256 id, 
            uint256 amount, 
            bytes memory toAddressBytes, 
            address fromAddress
        ) = abi.decode(_payload, (uint256, uint256, bytes, address));
        
        // Convert bytes to Base address
        address toAddress;
        assembly {
            toAddress := mload(add(toAddressBytes, 20))
        }
        
        // Check if destination address is valid
        require(toAddress != address(0), "Destination address cannot be zero address");
        
        // Mint tokens for recipient
        _mint(toAddress, id, amount, "");
        
        if (id == DMD_TOKEN_ID) {
            _totalSupply += amount;
            
            // Update recipient's tier cache
            updateUserTierCache(toAddress);
        }
        
        emit TokenReceived(_srcChainId, _srcAddress, id, amount, toAddress);
    }
    
    /**
     * @dev Set trusted remote for a chain
     * @param _chainId Chain ID in LayerZero
     * @param _remoteAddress Remote contract address
     */
    function setTrustedRemote(uint16 _chainId, bytes calldata _remoteAddress) external onlyOwner {
        _setTrustedRemote(_chainId, _remoteAddress);
    }

    /**
     * @dev Set trusted remote for Ethereum (convenience method)
     * @param _remoteAddress Remote contract address
     */
    function setTrustedRemoteETH(bytes calldata _remoteAddress) external onlyOwner {
        _setTrustedRemote(ETH_CHAIN_ID, _remoteAddress);
    }
    
    /**
     * @dev Set trusted remote for BSC (convenience method)
     * @param _remoteAddress Remote contract address
     */
    function setTrustedRemoteBSC(bytes calldata _remoteAddress) external onlyOwner {
        _setTrustedRemote(BSC_CHAIN_ID, _remoteAddress);
    }
    
    /**
     * @dev Set trusted remote for NEAR (convenience method)
     * @param _remoteAddress Remote contract address
     */
    function setTrustedRemoteNEAR(bytes calldata _remoteAddress) external onlyOwner {
        _setTrustedRemote(NEAR_CHAIN_ID, _remoteAddress);
    }
    
    /**
     * @dev Set trusted remote for Polygon (convenience method)
     * @param _remoteAddress Remote contract address
     */
    function setTrustedRemotePolygon(bytes calldata _remoteAddress) external onlyOwner {
        _setTrustedRemote(POLYGON_CHAIN_ID, _remoteAddress);
    }
    
    /**
     * @dev Set trusted remote for Arbitrum (convenience method)
     * @param _remoteAddress Remote contract address
     */
    function setTrustedRemoteArbitrum(bytes calldata _remoteAddress) external onlyOwner {
        _setTrustedRemote(ARBITRUM_CHAIN_ID, _remoteAddress);
    }
    
    /**
     * @dev Estimate fee needed for LayerZero to bridge tokens
     * @param chainId Destination chain ID in LayerZero
     * @param toAddress Destination address
     * @param id Token ID
     * @param amount Token amount
     * @return nativeFee Native token fee (ETH on Base)
     * @return zroFee ZRO token fee (always 0 as not used)
     */
    function estimateBridgeFee(uint16 chainId, bytes memory toAddress, uint256 id, uint256 amount) 
        public view returns (uint256 nativeFee, uint256 zroFee) 
    {
        bytes memory payload = abi.encode(id, amount, toAddress, msg.sender);
        return estimateFees(chainId, address(this), payload, false, bytes(""));
    }
    
    /**
     * @dev Estimate fee needed for LayerZero to bridge tokens to Ethereum
     * @param toAddress Destination address on Ethereum
     * @param id Token ID
     * @param amount Token amount
     * @return nativeFee Native token fee (ETH on Base)
     * @return zroFee ZRO token fee (always 0 as not used)
     */
    function estimateBridgeFeeToETH(bytes memory toAddress, uint256 id, uint256 amount) 
        public view returns (uint256 nativeFee, uint256 zroFee) 
    {
        return estimateBridgeFee(ETH_CHAIN_ID, toAddress, id, amount);
    }
    
    /**
     * @dev Estimate fee needed for LayerZero to bridge tokens to BSC
     * @param toAddress Destination address on BSC
     * @param id Token ID
     * @param amount Token amount
     * @return nativeFee Native token fee (ETH on Base)
     * @return zroFee ZRO token fee (always 0 as not used)
     */
    function estimateBridgeFeeToBSC(bytes memory toAddress, uint256 id, uint256 amount) 
        public view returns (uint256 nativeFee, uint256 zroFee) 
    {
        return estimateBridgeFee(BSC_CHAIN_ID, toAddress, id, amount);
    }
} 