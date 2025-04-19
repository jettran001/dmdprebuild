// SPDX-License-Identifier: MIT
pragma solidity 0.8.20;

import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/security/Pausable.sol";
import "@openzeppelin/contracts/security/ReentrancyGuard.sol";
import "@layerzerolabs/solidity-examples/contracts/lzApp/NonblockingLzApp.sol";

// Interface cho WrappedDMD token (ERC20)
interface IWrappedDMDToken {
    function burnWrapped(address from, uint256 amount) external;
    function mintWrapped(address to, uint256 amount) external;
}

/**
 * @title ERC1155 Bridge Adapter
 * @dev Adapter kết nối ERC1155Wrapper với bridge protocol (như LayerZero)
 * Format payload, gọi hàm gửi của bridge, và handle fee
 */
contract ERC1155BridgeAdapter is Ownable, Pausable, ReentrancyGuard, NonblockingLzApp {
    // Địa chỉ của WrappedDMD (ERC20)
    IWrappedDMDToken public wrappedDmd;
    
    // Chain IDs
    uint16 public constant NEAR_CHAIN_ID = 7;
    uint16 public constant SOLANA_CHAIN_ID = 8;
    
    // Phí bridge (tính bằng basis points, 1% = 100)
    uint256 public bridgeFeePercent = 30; // 0.3% ban đầu
    
    // Địa chỉ ví nhận phí
    address public feeCollector;
    
    // Chain được hỗ trợ
    mapping(uint16 => bool) public supportedChains;
    
    // Events
    event BridgeToNear(address indexed user, bytes nearAddress, uint256 amount, uint256 fee);
    event BridgeToSolana(address indexed user, bytes solanaAddress, uint256 amount, uint256 fee);
    event TokensReceived(uint16 srcChainId, bytes srcAddress, address to, uint256 amount);
    event BridgeFeeUpdated(uint256 newFee);
    
    /**
     * @dev Khởi tạo contract
     * @param _wrappedDmd Địa chỉ WrappedDMD ERC20
     * @param _lzEndpoint Địa chỉ của LayerZero endpoint
     */
    constructor(address _wrappedDmd, address _lzEndpoint) NonblockingLzApp(_lzEndpoint) {
        require(_wrappedDmd != address(0), "Wrapped DMD address cannot be zero");
        wrappedDmd = IWrappedDMDToken(_wrappedDmd);
        
        // Set supported chains
        supportedChains[NEAR_CHAIN_ID] = true;
        supportedChains[SOLANA_CHAIN_ID] = true;
        
        // Set fee collector to owner by default
        feeCollector = msg.sender;
    }
    
    /**
     * @dev Bridge WDMD token sang NEAR
     * @param from Địa chỉ người gửi token
     * @param nearAddress Địa chỉ nhận trên NEAR
     * @param amount Số lượng token
     */
    function bridgeToNear(address from, bytes calldata nearAddress, uint256 amount) external payable whenNotPaused nonReentrant {
        require(supportedChains[NEAR_CHAIN_ID], "NEAR bridging not supported");
        require(amount > 0, "Amount must be greater than 0");
        
        // Tính phí
        uint256 fee = (amount * bridgeFeePercent) / 10000;
        uint256 amountAfterFee = amount - fee;
        
        // Burn token
        wrappedDmd.burnWrapped(from, amount);
        
        // Encode payload
        bytes memory payload = abi.encode(from, nearAddress, amountAfterFee);
        
        // Estimate fee
        (uint256 lzFee, ) = _estimateFees(NEAR_CHAIN_ID, payload);
        require(msg.value >= lzFee, "Insufficient ETH for LayerZero fee");
        
        // Gửi message qua LayerZero
        _lzSend(NEAR_CHAIN_ID, payload, payable(msg.sender), address(0), bytes(""), msg.value);
        
        emit BridgeToNear(from, nearAddress, amountAfterFee, fee);
    }
    
    /**
     * @dev Bridge WDMD token sang Solana
     * @param from Địa chỉ người gửi token
     * @param solanaAddress Địa chỉ nhận trên Solana
     * @param amount Số lượng token
     */
    function bridgeToSolana(address from, bytes calldata solanaAddress, uint256 amount) external payable whenNotPaused nonReentrant {
        require(supportedChains[SOLANA_CHAIN_ID], "Solana bridging not supported");
        require(amount > 0, "Amount must be greater than 0");
        
        // Tính phí
        uint256 fee = (amount * bridgeFeePercent) / 10000;
        uint256 amountAfterFee = amount - fee;
        
        // Burn token
        wrappedDmd.burnWrapped(from, amount);
        
        // Encode payload
        bytes memory payload = abi.encode(from, solanaAddress, amountAfterFee);
        
        // Estimate fee
        (uint256 lzFee, ) = _estimateFees(SOLANA_CHAIN_ID, payload);
        require(msg.value >= lzFee, "Insufficient ETH for LayerZero fee");
        
        // Gửi message qua LayerZero
        _lzSend(SOLANA_CHAIN_ID, payload, payable(msg.sender), address(0), bytes(""), msg.value);
        
        emit BridgeToSolana(from, solanaAddress, amountAfterFee, fee);
    }
    
    /**
     * @dev Xử lý message nhận từ LayerZero
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
        require(supportedChains[_srcChainId], "Chain not supported");
        
        // Kiểm tra trusted remote
        require(_srcAddress.length > 0 && trustedRemotes[_srcChainId].length > 0, "Invalid source");
        require(keccak256(_srcAddress) == keccak256(trustedRemotes[_srcChainId]), "Source not trusted");
        
        // Decode payload
        (address to, uint256 amount) = abi.decode(_payload, (address, uint256));
        
        // Mint token
        wrappedDmd.mintWrapped(to, amount);
        
        emit TokensReceived(_srcChainId, _srcAddress, to, amount);
    }
    
    /**
     * @dev Ước tính phí
     * @param _dstChainId Chain ID đích
     * @param _payload Payload
     */
    function _estimateFees(uint16 _dstChainId, bytes memory _payload) internal view returns (uint256 nativeFee, uint256 zroFee) {
        return _lzEndpoint.estimateFees(_dstChainId, address(this), _payload, false, bytes(""));
    }
    
    /**
     * @dev Cập nhật phí bridge
     * @param _newFeePercent Phí mới (basis points)
     */
    function updateBridgeFee(uint256 _newFeePercent) external onlyOwner {
        require(_newFeePercent <= 500, "Fee too high"); // Max 5%
        bridgeFeePercent = _newFeePercent;
        emit BridgeFeeUpdated(_newFeePercent);
    }
    
    /**
     * @dev Cập nhật địa chỉ nhận phí
     * @param _newCollector Địa chỉ mới
     */
    function updateFeeCollector(address _newCollector) external onlyOwner {
        require(_newCollector != address(0), "Invalid address");
        feeCollector = _newCollector;
    }
    
    /**
     * @dev Cập nhật danh sách chain được hỗ trợ
     * @param _chainId Chain ID
     * @param _supported Trạng thái hỗ trợ
     */
    function updateSupportedChain(uint16 _chainId, bool _supported) external onlyOwner {
        supportedChains[_chainId] = _supported;
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
}
