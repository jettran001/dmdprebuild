// SPDX-License-Identifier: MIT
pragma solidity ^0.8.17;

// Interface cho ERC1155 để không cần import toàn bộ thư viện OpenZeppelin
interface IERC1155 {
    function balanceOf(address account, uint256 id) external view returns (uint256);
    function balanceOfBatch(address[] calldata accounts, uint256[] calldata ids) external view returns (uint256[] memory);
    function setApprovalForAll(address operator, bool approved) external;
    function isApprovedForAll(address account, address operator) external view returns (bool);
    function safeTransferFrom(address from, address to, uint256 id, uint256 amount, bytes calldata data) external;
    function safeBatchTransferFrom(address from, address to, uint256[] calldata ids, uint256[] calldata amounts, bytes calldata data) external;
}

// Interface cho ERC165
interface IERC165 {
    function supportsInterface(bytes4 interfaceId) external view returns (bool);
}

// Interface cho sự kiện ERC1155
interface IERC1155Receiver {
    function onERC1155Received(address operator, address from, uint256 id, uint256 value, bytes calldata data) external returns (bytes4);
    function onERC1155BatchReceived(address operator, address from, uint256[] calldata ids, uint256[] calldata values, bytes calldata data) external returns (bytes4);
}

// Interface cho sự kiện LayerZero nhận
interface ILayerZeroReceiver {
    function lzReceive(uint16 _srcChainId, bytes calldata _srcAddress, uint64 _nonce, bytes calldata _payload) external;
}

// Interface cho LayerZero Endpoint
interface ILayerZeroEndpoint {
    function send(uint16 _dstChainId, bytes calldata _destination, bytes calldata _payload, address payable _refundAddress, address _zroPaymentAddress, bytes calldata _adapterParams) external payable;
    function estimateFees(uint16 _dstChainId, address _userApplication, bytes calldata _payload, bool _payInZRO, bytes calldata _adapterParam) external view returns (uint256 nativeFee, uint256 zroFee);
}

/**
 * @title DiamondTokenEthereum
 * @dev DMD Token ERC-1155 standard on Ethereum với khả năng bridging qua LayerZero
 */
contract DiamondTokenEthereum {
    // Các sự kiện ERC1155
    event TransferSingle(address indexed operator, address indexed from, address indexed to, uint256 id, uint256 value);
    event TransferBatch(address indexed operator, address indexed from, address indexed to, uint256[] ids, uint256[] values);
    event ApprovalForAll(address indexed account, address indexed operator, bool approved);
    event URI(string value, uint256 indexed id);
    
    // Sự kiện riêng của DiamondToken
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
    
    // ID cho DMD fungible token (FT)
    uint256 public constant DMD_TOKEN_ID = 0;
    
    // Base metadata URI
    string private _baseURI;
    
    // Token name và symbol
    string public name;
    string public symbol;
    
    // Token decimals
    uint8 public decimals;
    
    // Mapping để lưu trữ URI metadata cho NFTs
    mapping(uint256 => string) private _tokenURIs;
    
    // Tổng cung của DMD token
    uint256 private _totalSupply;
    
    // LayerZero endpoint chain IDs
    uint16 public constant BASE_CHAIN_ID = 184;   // LayerZero chain ID cho Base
    uint16 public constant BSC_CHAIN_ID = 102;    // LayerZero chain ID cho BSC
    uint16 public constant NEAR_CHAIN_ID = 115;   // LayerZero chain ID cho NEAR
    uint16 public constant POLYGON_CHAIN_ID = 109; // LayerZero chain ID cho Polygon
    uint16 public constant ARBITRUM_CHAIN_ID = 110; // LayerZero chain ID cho Arbitrum
    
    // Enum định nghĩa các tier cho DMD token
    enum DMDTier {
        Regular,     // Regular Fungible Token (< 1000)
        Bronze,      // VIP Badge tier Bronze (>= 1000)
        Silver,      // VIP Badge tier Silver (>= 5000)
        Gold,        // VIP Badge tier Gold (>= 10000)
        Diamond      // VIP Badge tier Diamond (>= 30000)
    }
    
    // Ngưỡng lượng token cho mỗi tier
    uint256 public bronzeThreshold = 1000 * 10**18;  // 1,000 DMD
    uint256 public silverThreshold = 5000 * 10**18;  // 5,000 DMD
    uint256 public goldThreshold = 10000 * 10**18;   // 10,000 DMD
    uint256 public diamondThreshold = 30000 * 10**18; // 30,000 DMD
    
    // Mảng lưu trữ các tier
    DMDTier[] public tiers;
    
    // Mappings để lưu trữ thông tin tier tùy chỉnh
    mapping(uint8 => uint256) public customTierThresholds;
    mapping(uint8 => string) public customTierNames;
    mapping(uint8 => string) public customTierDescriptions;
    mapping(uint8 => string) public customTierURIs;
    
    // Cache lưu trữ tier người dùng để tối ưu gas
    mapping(address => DMDTier) private _userTierCache;
    // Thời gian hết hạn cache (tính bằng giây)
    mapping(address => uint256) private _userTierCacheExpiry;
    // Cache time-to-live (mặc định: 1 giờ)
    uint256 public tierCacheTTL = 1 hours;
    
    // Tier URIs cho mỗi loại tier
    string public regularTierURI;
    string public bronzeTierURI;
    string public silverTierURI;
    string public goldTierURI;
    string public diamondTierURI;
    
    // Mapping của balance
    mapping(address => mapping(uint256 => uint256)) private _balances;
    
    // Mapping của approval
    mapping(address => mapping(address => bool)) private _operatorApprovals;
    
    // LayerZero Endpoint
    ILayerZeroEndpoint public endpoint;
    
    // Mapping trusted remotes
    mapping(uint16 => bytes) public trustedRemotes;
    
    // Owner address
    address public owner;
    
    // Pausable state
    bool public paused;
    
    // Modifiers
    modifier onlyOwner() {
        require(msg.sender == owner, "Only owner can call");
        _;
    }
    
    modifier whenNotPaused() {
        require(!paused, "Contract is paused");
        _;
    }
    
    /**
     * @dev Khởi tạo contract
     * @param baseURI Base URI cho metadata
     * @param lzEndpoint Address của LayerZero Endpoint trên Ethereum
     */
    constructor(string memory baseURI, address lzEndpoint) {
        owner = msg.sender;
        name = "Diamond Token";
        symbol = "DMD";
        decimals = 18;
        _baseURI = baseURI;
        endpoint = ILayerZeroEndpoint(lzEndpoint);
        
        // Khởi tạo tổng cung DMD: 1 tỷ token (18 decimals)
        _totalSupply = 1_000_000_000 * 10**decimals;
        
        // Khởi tạo URIs cho các tier
        regularTierURI = string(abi.encodePacked(baseURI, "/regular"));
        bronzeTierURI = string(abi.encodePacked(baseURI, "/bronze"));
        silverTierURI = string(abi.encodePacked(baseURI, "/silver"));
        goldTierURI = string(abi.encodePacked(baseURI, "/gold"));
        diamondTierURI = string(abi.encodePacked(baseURI, "/diamond"));
        
        // Khởi tạo danh sách tier
        tiers.push(DMDTier.Regular);
        tiers.push(DMDTier.Bronze);
        tiers.push(DMDTier.Silver);
        tiers.push(DMDTier.Gold);
        tiers.push(DMDTier.Diamond);
        
        // Mint toàn bộ token cho creator của contract
        _balances[msg.sender][DMD_TOKEN_ID] = _totalSupply;
        emit TransferSingle(msg.sender, address(0), msg.sender, DMD_TOKEN_ID, _totalSupply);
    }
    
    /**
     * @dev Thay đổi base URI
     * @param newuri URI mới
     */
    function setURI(string memory newuri) public onlyOwner {
        _baseURI = newuri;
    }
    
    /**
     * @dev Lấy URI của token
     * @param tokenId ID của token
     * @return Metadata URI của token
     */
    function uri(uint256 tokenId) public view returns (string memory) {
        // Nếu là NFT có URI riêng
        if (tokenId != DMD_TOKEN_ID && bytes(_tokenURIs[tokenId]).length > 0) {
            return _tokenURIs[tokenId];
        }
        
        // Nếu là DMD token (ID = 0), trả về URI dựa trên tier
        if (tokenId == DMD_TOKEN_ID) {
            // Nếu địa chỉ trống, trả về URI mặc định
            if (msg.sender == address(0)) {
                return _baseURI;
            }
            
            // Xác định tier dựa trên số dư
            DMDTier tier = getUserTier(msg.sender);
            
            // Trả về URI tương ứng với tier
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
        
        // Cho các NFT khác
        return string(abi.encodePacked(_baseURI, _toString(tokenId)));
    }
    
    /**
     * @dev Lấy balance của tài khoản
     * @param account Địa chỉ tài khoản
     * @param id ID của token
     * @return Số dư token
     */
    function balanceOf(address account, uint256 id) public view returns (uint256) {
        require(account != address(0), "ERC1155: balance query for the zero address");
        return _balances[account][id];
    }
    
    /**
     * @dev Lấy balance của nhiều tài khoản
     * @param accounts Mảng địa chỉ tài khoản
     * @param ids Mảng ID token
     * @return Mảng số dư token
     */
    function balanceOfBatch(address[] memory accounts, uint256[] memory ids) public view returns (uint256[] memory) {
        require(accounts.length == ids.length, "ERC1155: accounts and ids length mismatch");

        uint256[] memory batchBalances = new uint256[](accounts.length);

        for (uint256 i = 0; i < accounts.length; ++i) {
            batchBalances[i] = balanceOf(accounts[i], ids[i]);
        }

        return batchBalances;
    }
    
    /**
     * @dev Thiết lập approval cho operator
     * @param operator Địa chỉ được phép thao tác
     * @param approved Trạng thái approval
     */
    function setApprovalForAll(address operator, bool approved) public {
        require(msg.sender != operator, "ERC1155: setting approval status for self");
        _operatorApprovals[msg.sender][operator] = approved;
        emit ApprovalForAll(msg.sender, operator, approved);
    }
    
    /**
     * @dev Kiểm tra approval 
     * @param account Địa chỉ tài khoản
     * @param operator Địa chỉ operator
     * @return Trạng thái approval
     */
    function isApprovedForAll(address account, address operator) public view returns (bool) {
        return _operatorApprovals[account][operator];
    }
    
    /**
     * @dev Chuyển token an toàn
     * @param from Địa chỉ gửi
     * @param to Địa chỉ nhận
     * @param id ID token
     * @param amount Số lượng
     * @param data Data bổ sung
     */
    function safeTransferFrom(address from, address to, uint256 id, uint256 amount, bytes memory data) public whenNotPaused {
        require(
            from == msg.sender || isApprovedForAll(from, msg.sender),
            "ERC1155: caller is not owner nor approved"
        );
        require(to != address(0), "ERC1155: transfer to the zero address");

        _balances[from][id] -= amount;
        _balances[to][id] += amount;

        // Cập nhật tier cache
        if (id == DMD_TOKEN_ID) {
            invalidateTierCache(from);
            updateUserTierCache(to);
        }

        emit TransferSingle(msg.sender, from, to, id, amount);
        
        // Gọi onERC1155Received nếu người nhận là contract
        if (_isContract(to)) {
            try IERC1155Receiver(to).onERC1155Received(msg.sender, from, id, amount, data) returns (bytes4 response) {
                if (response != IERC1155Receiver.onERC1155Received.selector) {
                    revert("ERC1155: ERC1155Receiver rejected tokens");
                }
            } catch Error(string memory reason) {
                revert(reason);
            } catch {
                revert("ERC1155: transfer to non ERC1155Receiver implementer");
            }
        }
    }
    
    /**
     * @dev Cập nhật tier cache của người dùng
     * @param user Địa chỉ người dùng
     */
    function updateUserTierCache(address user) internal {
        DMDTier tier = getUserTier(user);
        _userTierCache[user] = tier;
        _userTierCacheExpiry[user] = block.timestamp + tierCacheTTL;
    }
    
    /**
     * @dev Xóa tier cache
     * @param user Địa chỉ người dùng cần xóa cache
     */
    function invalidateTierCache(address user) public {
        require(
            user == msg.sender || owner == msg.sender,
            "Only owner or user can invalidate cache"
        );
        delete _userTierCache[user];
        delete _userTierCacheExpiry[user];
    }
    
    /**
     * @dev Xác định tier người dùng dựa trên số dư DMD
     * @param user Địa chỉ người dùng
     * @return Tier của người dùng
     */
    function getUserTier(address user) public view returns (DMDTier) {
        // Kiểm tra cache nếu còn hạn
        if (_userTierCacheExpiry[user] > block.timestamp) {
            return _userTierCache[user];
        }
        
        uint256 balance = balanceOf(user, DMD_TOKEN_ID);
        
        // Kiểm tra các tier tiêu chuẩn trước
        if (balance >= diamondThreshold) {
            return DMDTier.Diamond;
        } else if (balance >= goldThreshold) {
            return DMDTier.Gold;
        } else if (balance >= silverThreshold) {
            return DMDTier.Silver;
        } else if (balance >= bronzeThreshold) {
            return DMDTier.Bronze;
        }
        
        // Kiểm tra các tier tùy chỉnh
        for (uint8 i = 5; i < tiers.length; i++) {
            if (balance >= customTierThresholds[i]) {
                return DMDTier(i);
            }
        }
        
        return DMDTier.Regular;
    }
    
    /**
     * @dev Bridge token đến một chain khác
     * @param chainId ID chain đích trong LayerZero
     * @param toAddress Địa chỉ đích trên chain đích
     * @param id ID token cần bridge
     * @param amount Số lượng token
     */
    function bridgeToOtherChain(uint16 chainId, bytes memory toAddress, uint256 id, uint256 amount) public payable whenNotPaused {
        require(balanceOf(msg.sender, id) >= amount, "Insufficient token balance");
        require(chainId != 0, "Invalid destination chain ID");
        require(trustedRemotes[chainId].length > 0, "Destination chain not trusted");
        
        // Burn token trước khi chuyển
        _burn(msg.sender, id, amount);
        
        // Tạo payload để gửi qua LayerZero
        bytes memory payload = abi.encode(id, amount, toAddress, msg.sender);
        
        // Gửi message qua LayerZero
        endpoint.send{value: msg.value}(
            chainId,                 // Chain ID đích trong LayerZero
            trustedRemotes[chainId], // Địa chỉ đích trên chain đích
            payload,                 // Data để gửi
            payable(msg.sender),     // Địa chỉ để refund
            address(0x0),            // _zroPaymentAddress (0x0 = không dùng ZRO token cho payment)
            bytes("")                // _adapterParams
        );
        
        emit TokenBridged(msg.sender, id, amount, chainId, toAddress);
    }
    
    /**
     * @dev Nhận token từ LayerZero (từ chain khác)
     * @param _srcChainId Chain ID nguồn
     * @param _srcAddress Địa chỉ nguồn
     * @param _nonce Nonce
     * @param _payload Payload
     */
    function lzReceive(uint16 _srcChainId, bytes memory _srcAddress, uint64 _nonce, bytes memory _payload) external {
        // Chỉ cho phép endpoint gọi vào hàm này
        require(msg.sender == address(endpoint), "Only LayerZero endpoint can call this function");
        
        // Kiểm tra trusted remote
        bytes memory srcAddrBytes = abi.encodePacked(_srcChainId, _srcAddress);
        require(trustedRemotes[_srcChainId].length > 0, "Source chain not trusted");
        require(_srcAddress.length == trustedRemotes[_srcChainId].length, "Invalid source address");
        
        // Kiểm tra trusted address
        uint256 sourceAddressLength = _srcAddress.length;
        for (uint256 i = 0; i < sourceAddressLength; i++) {
            if (_srcAddress[i] != trustedRemotes[_srcChainId][i]) {
                revert("Source address not trusted");
            }
        }
        
        // Giải mã payload
        (
            uint256 id, 
            uint256 amount, 
            bytes memory toAddressBytes, 
            address fromAddress
        ) = abi.decode(_payload, (uint256, uint256, bytes, address));
        
        // Chuyển đổi bytes thành Ethereum address
        address toAddress;
        assembly {
            toAddress := mload(add(toAddressBytes, 20))
        }
        
        // Kiểm tra xem địa chỉ đích có hợp lệ không
        require(toAddress != address(0), "Destination address cannot be zero address");
        
        // Mint token cho người nhận
        _mint(toAddress, id, amount, "");
        
        emit TokenReceived(_srcChainId, _srcAddress, id, amount, toAddress);
    }
    
    /**
     * @dev Ước tính phí cần thiết cho LayerZero để bridge token
     * @param chainId Chain ID đích trong LayerZero
     * @param toAddress Địa chỉ đích
     * @param id ID token
     * @param amount Số lượng token
     * @return nativeFee Phí native token (ETH trên Ethereum)
     * @return zroFee Phí ZRO token (luôn là 0 vì không sử dụng)
     */
    function estimateBridgeFee(uint16 chainId, bytes memory toAddress, uint256 id, uint256 amount) 
        public view returns (uint256 nativeFee, uint256 zroFee) 
    {
        bytes memory payload = abi.encode(id, amount, toAddress, msg.sender);
        return endpoint.estimateFees(chainId, address(this), payload, false, bytes(""));
    }
    
    /**
     * @dev Pause contract
     */
    function pause() public onlyOwner {
        paused = true;
    }
    
    /**
     * @dev Unpause contract
     */
    function unpause() public onlyOwner {
        paused = false;
    }
    
    /**
     * @dev Set trusted remote cho một chain
     * @param _chainId Chain ID trong LayerZero
     * @param _remoteAddress Địa chỉ remote contract
     */
    function setTrustedRemote(uint16 _chainId, bytes calldata _remoteAddress) external onlyOwner {
        trustedRemotes[_chainId] = _remoteAddress;
    }
    
    /**
     * @dev Mint DMD tokens
     * @param to Địa chỉ nhận token
     * @param amount Số lượng token
     */
    function mintDMD(address to, uint256 amount) public onlyOwner {
        require(to != address(0), "ERC1155: mint to the zero address");
        
        _balances[to][DMD_TOKEN_ID] += amount;
        _totalSupply += amount;
        
        // Update tier cache
        updateUserTierCache(to);
        
        emit TransferSingle(msg.sender, address(0), to, DMD_TOKEN_ID, amount);
        emit TokenMinted(to, DMD_TOKEN_ID, amount);
    }
    
    /**
     * @dev Burn DMD tokens
     * @param from Địa chỉ burn token
     * @param amount Số lượng token
     */
    function burn(address from, uint256 id, uint256 amount) public {
        require(
            from == msg.sender || isApprovedForAll(from, msg.sender),
            "ERC1155: caller is not owner nor approved"
        );
        
        _burn(from, id, amount);
    }
    
    // Internal functions
    
    /**
     * @dev Internal mint
     */
    function _mint(address to, uint256 id, uint256 amount, bytes memory data) internal {
        require(to != address(0), "ERC1155: mint to the zero address");
        
        _balances[to][id] += amount;
        
        if (id == DMD_TOKEN_ID) {
            _totalSupply += amount;
            updateUserTierCache(to);
        }
        
        emit TransferSingle(msg.sender, address(0), to, id, amount);
    }
    
    /**
     * @dev Internal burn
     */
    function _burn(address from, uint256 id, uint256 amount) internal {
        require(from != address(0), "ERC1155: burn from the zero address");
        require(_balances[from][id] >= amount, "ERC1155: burn amount exceeds balance");
        
        _balances[from][id] -= amount;
        
        if (id == DMD_TOKEN_ID) {
            _totalSupply -= amount;
            invalidateTierCache(from);
        }
        
        emit TransferSingle(msg.sender, from, address(0), id, amount);
        emit TokenBurned(from, id, amount);
    }
    
    /**
     * @dev Kiểm tra xem một địa chỉ có phải là contract không
     */
    function _isContract(address account) internal view returns (bool) {
        uint256 size;
        assembly {
            size := extcodesize(account)
        }
        return size > 0;
    }
    
    /**
     * @dev Chuyển đổi uint256 thành string
     */
    function _toString(uint256 value) internal pure returns (string memory) {
        if (value == 0) {
            return "0";
        }
        
        uint256 temp = value;
        uint256 digits;
        
        while (temp != 0) {
            digits++;
            temp /= 10;
        }
        
        bytes memory buffer = new bytes(digits);
        
        while (value != 0) {
            digits -= 1;
            buffer[digits] = bytes1(uint8(48 + uint256(value % 10)));
            value /= 10;
        }
        
        return string(buffer);
    }
} 