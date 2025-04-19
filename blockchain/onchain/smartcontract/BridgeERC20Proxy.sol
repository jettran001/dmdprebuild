// SPDX-License-Identifier: MIT
pragma solidity 0.8.17;

import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/security/Pausable.sol";
import "@openzeppelin/contracts/security/ReentrancyGuard.sol";
import "@layerzerolabs/solidity-examples/contracts/lzApp/NonblockingLzApp.sol";

// Interface cho ERC20DMD
interface IERC20DMD {
    function mintWrapped(address to, uint256 amount) external;
    function burnWrapped(address from, uint256 amount) external;
    function balanceOf(address account) external view returns (uint256);
    function transfer(address to, uint256 amount) external returns (bool);
    function transferFrom(address from, address to, uint256 amount) external returns (bool);
}

// Interface cho DiamondToken
interface IDiamondToken {
    function unwrapFromERC20(address to, uint256 amount) external;
    function bridgedSupply() external view returns (uint256);
    function DMD_TOKEN_ID() external view returns (uint256);
}

/**
 * @title BridgeERC20Proxy
 * @dev Proxy contract để xử lý bridge giữa ERC20DMD và các blockchain khác
 * Sử dụng LayerZero để bridge với NEAR và các chain khác
 */
contract BridgeERC20Proxy is Ownable, Pausable, ReentrancyGuard, NonblockingLzApp {
    // ERC20DMD token
    IERC20DMD public erc20Token;
    
    // DiamondToken (ERC1155)
    IDiamondToken public dmdToken;
    
    // LayerZero chain IDs
    uint16 public constant NEAR_CHAIN_ID = 115;
    uint16 public constant SOLANA_CHAIN_ID = 168;
    uint16 public constant ETHEREUM_CHAIN_ID = 1;
    uint16 public constant POLYGON_CHAIN_ID = 109;
    uint16 public constant SUI_CHAIN_ID = 110;
    uint16 public constant PI_CHAIN_ID = 111;
    
    // Phí bridge (basis points: 1% = 100)
    uint16 public bridgeFeePercent = 10; // 0.1% mặc định
    
    // Địa chỉ nhận phí bridge
    address public feeCollector;
    
    // Trusted remote addresses
    mapping(uint16 => bytes) public trustedRemotes;
    
    // Các chain được hỗ trợ
    mapping(uint16 => bool) public supportedChains;
    
    // Events
    event BridgeToChain(address indexed from, uint16 destChain, bytes destAddress, uint256 amount, uint256 fee);
    event TokensReceived(uint16 indexed fromChainId, address toAddress, uint256 amount, string actionType);
    event TrustedRemoteUpdated(uint16 indexed chainId, bytes remoteAddress);
    event SupportedChainUpdated(uint16 indexed chainId, bool supported);
    event TokensSet(address indexed erc20Token, address indexed dmdToken);
    event BridgeFeeUpdated(uint16 newFeePercent);
    event FeeCollectorUpdated(address newFeeCollector);
    
    /**
     * @dev Khởi tạo contract
     * @param _lzEndpoint Địa chỉ LayerZero Endpoint
     * @param _erc20Token Địa chỉ ERC20DMD token
     * @param _dmdToken Địa chỉ DiamondToken (ERC1155)
     */
    constructor(address _lzEndpoint, address _erc20Token, address _dmdToken) NonblockingLzApp(_lzEndpoint) {
        require(_erc20Token != address(0), "ERC20 token không thể là zero address");
        require(_dmdToken != address(0), "DMD token không thể là zero address");
        
        erc20Token = IERC20DMD(_erc20Token);
        dmdToken = IDiamondToken(_dmdToken);
        feeCollector = msg.sender;
        
        // Mặc định hỗ trợ NEAR
        supportedChains[NEAR_CHAIN_ID] = true;
        
        emit TokensSet(_erc20Token, _dmdToken);
    }
    
    /**
     * @dev Bridge ERC20 sang NEAR
     * @param from Địa chỉ người gửi
     * @param nearAddress Địa chỉ NEAR dạng bytes
     * @param amount Số lượng token
     */
    function bridgeToNear(address from, bytes calldata nearAddress, uint256 amount) external payable whenNotPaused nonReentrant {
        bridgeToChain(from, NEAR_CHAIN_ID, nearAddress, amount);
    }
    
    /**
     * @dev Bridge ERC20 sang chain khác
     * @param from Địa chỉ người gửi
     * @param destChainId Chain ID đích
     * @param destAddress Địa chỉ đích dạng bytes
     * @param amount Số lượng token
     */
    function bridgeToChain(address from, uint16 destChainId, bytes calldata destAddress, uint256 amount) public payable whenNotPaused nonReentrant {
        require(supportedChains[destChainId], "Chain không được hỗ trợ");
        require(trustedRemotes[destChainId].length > 0, "Trusted remote chưa được cấu hình");
        require(amount > 0, "Số lượng token phải lớn hơn 0");
        
        // Tính phí bridge
        uint256 fee = (amount * bridgeFeePercent) / 10000;
        uint256 amountAfterFee = amount - fee;
        
        // Burn token ERC20
        erc20Token.burnWrapped(address(this), amount);
        
        // Chuẩn bị payload
        bytes memory payload = abi.encode(from, amountAfterFee, "wrap");
        
        // Ước tính phí LayerZero
        (uint256 lzFee, ) = _estimateFees(destChainId, payload);
        require(msg.value >= lzFee, "Không đủ ETH để trả phí LayerZero");
        
        // Gửi message qua LayerZero
        _lzSend(
            destChainId,
            trustedRemotes[destChainId],
            payload,
            payable(from),
            address(0),
            bytes(""),
            msg.value
        );
        
        emit BridgeToChain(from, destChainId, destAddress, amountAfterFee, fee);
    }
    
    /**
     * @dev Xử lý message từ LayerZero
     * @param _srcChainId Chain ID nguồn
     * @param _srcAddress Địa chỉ nguồn
     * @param _nonce Nonce
     * @param _payload Payload
     */
    function _nonblockingLzReceive(
        uint16 _srcChainId,
        bytes memory _srcAddress,
        uint64 _nonce,
        bytes memory _payload
    ) internal override {
        require(supportedChains[_srcChainId], "Chain không được hỗ trợ");
        
        // Xác minh trusted remote
        require(_srcAddress.length > 0 && trustedRemotes[_srcChainId].length > 0, "Nguồn không hợp lệ");
        require(keccak256(_srcAddress) == keccak256(trustedRemotes[_srcChainId]), "Nguồn không tin cậy");
        
        // Giải mã payload
        (address toAddress, uint256 amount, string memory actionType) = abi.decode(_payload, (address, uint256, string));
        
        if (keccak256(bytes(actionType)) == keccak256(bytes("direct_erc1155"))) {
            // Mint trực tiếp vào ERC1155
            dmdToken.unwrapFromERC20(toAddress, amount);
        } else {
            // Mặc định: mint vào ERC20
            erc20Token.mintWrapped(toAddress, amount);
        }
        
        emit TokensReceived(_srcChainId, toAddress, amount, actionType);
    }
    
    /**
     * @dev Ước tính phí bridge
     * @param _dstChainId Chain ID đích
     * @param _payload Payload
     * @return nativeFee Phí native
     * @return zroFee Phí ZRO
     */
    function _estimateFees(uint16 _dstChainId, bytes memory _payload) internal view returns (uint256 nativeFee, uint256 zroFee) {
        return _lzEndpoint.estimateFees(_dstChainId, address(this), _payload, false, bytes(""));
    }
    
    /**
     * @dev Ước tính phí bridge (public)
     * @param _dstChainId Chain ID đích
     * @return Tổng phí
     */
    function estimateBridgeFee(uint16 _dstChainId) external view returns (uint256) {
        bytes memory payload = abi.encode(address(0), uint256(0), "wrap");
        (uint256 nativeFee, ) = _estimateFees(_dstChainId, payload);
        return nativeFee;
    }
    
    /**
     * @dev Thiết lập trusted remote cho chain
     * @param _chainId Chain ID
     * @param _remoteAddress Địa chỉ remote
     */
    function setTrustedRemote(uint16 _chainId, bytes calldata _remoteAddress) external onlyOwner {
        trustedRemotes[_chainId] = _remoteAddress;
        emit TrustedRemoteUpdated(_chainId, _remoteAddress);
    }
    
    /**
     * @dev Thiết lập chain được hỗ trợ
     * @param _chainId Chain ID
     * @param _supported Trạng thái hỗ trợ
     */
    function setSupportedChain(uint16 _chainId, bool _supported) external onlyOwner {
        supportedChains[_chainId] = _supported;
        emit SupportedChainUpdated(_chainId, _supported);
    }
    
    /**
     * @dev Thiết lập phí bridge
     * @param _feePercent Phần trăm phí (basis points)
     */
    function setBridgeFee(uint16 _feePercent) external onlyOwner {
        require(_feePercent <= 1000, "Phí không được vượt quá 10%");
        bridgeFeePercent = _feePercent;
        emit BridgeFeeUpdated(_feePercent);
    }
    
    /**
     * @dev Thiết lập địa chỉ nhận phí
     * @param _feeCollector Địa chỉ nhận phí
     */
    function setFeeCollector(address _feeCollector) external onlyOwner {
        require(_feeCollector != address(0), "Địa chỉ nhận phí không thể là zero address");
        feeCollector = _feeCollector;
        emit FeeCollectorUpdated(_feeCollector);
    }
    
    /**
     * @dev Tạm dừng bridge
     */
    function pause() external onlyOwner {
        _pause();
    }
    
    /**
     * @dev Mở lại bridge
     */
    function unpause() external onlyOwner {
        _unpause();
    }
    
    /**
     * @dev Rút phí đã thu
     * @param _token Địa chỉ token (zero address cho ETH)
     * @param _amount Số lượng cần rút
     */
    function withdrawFees(address _token, uint256 _amount) external onlyOwner {
        if (_token == address(0)) {
            (bool success, ) = feeCollector.call{value: _amount}("");
            require(success, "Chuyển ETH thất bại");
        } else {
            // Xử lý token khác nếu cần
        }
    }
    
    /**
     * @dev Nhận ETH
     */
    receive() external payable {}
} 