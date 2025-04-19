// SPDX-License-Identifier: MIT
pragma solidity 0.8.20;

import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/security/Pausable.sol";
import "@openzeppelin/contracts/utils/Counters.sol";
import "@openzeppelin/contracts/token/ERC20/IERC20.sol";

/**
 * @title IWrappedDMD
 * @dev Interface cho WrappedDMD token
 */
interface IWrappedDMD {
    function mint(address to, uint256 amount) external;
    function burnFrom(address account, uint256 amount) external;
    function balanceOf(address account) external view returns (uint256);
}

/**
 * @title IDiamondToken
 * @dev Interface cho DMD token trên BSC (ERC1155)
 */
interface IDiamondToken {
    function safeTransferFrom(address from, address to, uint256 id, uint256 amount, bytes calldata data) external;
    function balanceOf(address account, uint256 id) external view returns (uint256);
}

/**
 * @title IBridgeAdapter
 * @dev Interface cho Bridge Adapter
 */
interface IBridgeAdapter {
    function bridgeTo(uint16 dstChainId, bytes calldata destination, uint256 amount) external payable;
    function estimateFee(uint16 dstChainId, uint256 amount) external view returns (uint256);
    function supportedChains() external view returns (uint16[] memory);
    function adapterType() external view returns (string memory);
    function targetChain() external view returns (uint16);
}

/**
 * @title BridgeInterface
 * @dev BridgeInterface là router đa bridge quản lý các bridge adapter và điều hướng giao dịch qua adapter phù hợp
 */
contract BridgeInterface is Ownable, Pausable {
    using Counters for Counters.Counter;
    
    // Các sự kiện
    event BridgeRegistered(address indexed adapter, string adapterType, uint16 targetChain);
    event BridgeRemoved(address indexed adapter);
    event BridgeUsed(address indexed user, address indexed adapter, uint16 targetChain, uint256 amount);
    event FeeUpdated(uint256 feePercentage);
    event FeeCollectorUpdated(address indexed newCollector);
    
    // Biến số
    Counters.Counter private bridgeCounter;
    mapping(address => bool) public registeredBridges;
    mapping(uint16 => address[]) public bridgeAdapters; // chainId => array of bridge adapters
    mapping(address => uint16) public adapterChains;   // adapter => chainId
    mapping(address => string) public adapterTypes;    // adapter => type
    
    // Cài đặt phí
    uint256 public feePercentage = 0;  // 0.1% = 1, 1% = 10
    uint256 public constant MAX_FEE = 100; // tối đa 10%
    address public feeCollector;
    
    /**
     * @dev Khởi tạo BridgeInterface
     * @param _feeCollector Địa chỉ thu phí
     */
    constructor(address _feeCollector) {
        feeCollector = _feeCollector;
    }
    
    /**
     * @dev Đăng ký một bridge adapter mới
     * @param adapter Địa chỉ của bridge adapter
     */
    function registerBridge(address adapter) external onlyOwner {
        require(adapter != address(0), "Invalid adapter address");
        require(!registeredBridges[adapter], "Bridge already registered");
        
        string memory adapterType = IBridgeAdapter(adapter).adapterType();
        uint16 chainId = IBridgeAdapter(adapter).targetChain();
        
        registeredBridges[adapter] = true;
        bridgeAdapters[chainId].push(adapter);
        adapterChains[adapter] = chainId;
        adapterTypes[adapter] = adapterType;
        
        bridgeCounter.increment();
        
        emit BridgeRegistered(adapter, adapterType, chainId);
    }
    
    /**
     * @dev Hủy đăng ký một bridge adapter
     * @param adapter Địa chỉ của bridge adapter
     */
    function removeBridge(address adapter) external onlyOwner {
        require(registeredBridges[adapter], "Bridge not registered");
        
        uint16 chainId = adapterChains[adapter];
        registeredBridges[adapter] = false;
        
        // Xóa adapter khỏi mảng
        for (uint i = 0; i < bridgeAdapters[chainId].length; i++) {
            if (bridgeAdapters[chainId][i] == adapter) {
                // Thay adapter cần xóa bằng adapter cuối cùng
                bridgeAdapters[chainId][i] = bridgeAdapters[chainId][bridgeAdapters[chainId].length - 1];
                // Loại bỏ phần tử cuối
                bridgeAdapters[chainId].pop();
                break;
            }
        }
        
        bridgeCounter.decrement();
        
        emit BridgeRemoved(adapter);
    }
    
    /**
     * @dev Lấy số lượng bridge adapter đã đăng ký
     * @return Số lượng bridge
     */
    function getBridgeCount() external view returns (uint256) {
        return bridgeCounter.current();
    }
    
    /**
     * @dev Lấy danh sách adapter cho một chain cụ thể
     * @param chainId ID của chain
     * @return Danh sách địa chỉ adapter
     */
    function getAdaptersForChain(uint16 chainId) external view returns (address[] memory) {
        return bridgeAdapters[chainId];
    }
    
    /**
     * @dev Tính phí bridge
     * @param amount Số lượng token chuyển
     * @return Phí bridge
     */
    function calculateFee(uint256 amount) public view returns (uint256) {
        return amount * feePercentage / 1000;
    }
    
    /**
     * @dev Gửi token sử dụng adapter cụ thể
     * @param adapter Địa chỉ của bridge adapter
     * @param dstChainId ID của chain đích
     * @param destination Địa chỉ đích đã mã hóa
     * @param amount Số lượng token gửi
     */
    function sendMessage(address adapter, uint16 dstChainId, bytes calldata destination, uint256 amount) 
        external
        payable
        whenNotPaused
    {
        require(registeredBridges[adapter], "Bridge not registered");
        require(adapterChains[adapter] == dstChainId, "Chain ID mismatch");
        
        // Tính phí
        uint256 fee = calculateFee(amount);
        uint256 amountAfterFee = amount - fee;
        
        // Gửi phí cho collector
        if (fee > 0 && feeCollector != address(0)) {
            IWrappedDMD(getTokenAddress()).burnFrom(msg.sender, fee);
            IWrappedDMD(getTokenAddress()).mint(feeCollector, fee);
        }
        
        // Gửi token qua bridge
        IWrappedDMD(getTokenAddress()).burnFrom(msg.sender, amountAfterFee);
        
        // Gọi bridge adapter
        uint256 adapterFee = IBridgeAdapter(adapter).estimateFee(dstChainId, amountAfterFee);
        require(msg.value >= adapterFee, "Insufficient fee");
        
        IBridgeAdapter(adapter).bridgeTo{value: msg.value}(dstChainId, destination, amountAfterFee);
        
        emit BridgeUsed(msg.sender, adapter, dstChainId, amountAfterFee);
    }
    
    /**
     * @dev Tự động chọn bridge thích hợp cho chain đích
     * @param dstChainId ID của chain đích
     * @param destination Địa chỉ đích đã mã hóa
     * @param amount Số lượng token gửi
     */
    function autoBridge(uint16 dstChainId, bytes calldata destination, uint256 amount) 
        external
        payable
        whenNotPaused
    {
        require(bridgeAdapters[dstChainId].length > 0, "No adapter for destination chain");
        
        // Tìm bridge có phí thấp nhất
        address bestAdapter = address(0);
        uint256 lowestFee = type(uint256).max;
        
        for (uint i = 0; i < bridgeAdapters[dstChainId].length; i++) {
            address adapter = bridgeAdapters[dstChainId][i];
            uint256 adapterFee = IBridgeAdapter(adapter).estimateFee(dstChainId, amount);
            
            if (adapterFee < lowestFee) {
                lowestFee = adapterFee;
                bestAdapter = adapter;
            }
        }
        
        require(msg.value >= lowestFee, "Insufficient fee");
        
        // Gọi sendMessage với adapter đã chọn
        this.sendMessage{value: msg.value}(bestAdapter, dstChainId, destination, amount);
    }
    
    /**
     * @dev Ước tính phí cho chain đích và adapter cụ thể
     * @param adapter Địa chỉ của bridge adapter
     * @param dstChainId ID của chain đích
     * @param amount Số lượng token gửi
     * @return Tổng phí (gồm phí bridge và phí protocol)
     */
    function estimateAdapterFee(address adapter, uint16 dstChainId, uint256 amount) 
        external
        view
        returns (uint256)
    {
        require(registeredBridges[adapter], "Bridge not registered");
        
        uint256 protocolFee = calculateFee(amount);
        uint256 adapterFee = IBridgeAdapter(adapter).estimateFee(dstChainId, amount - protocolFee);
        
        return protocolFee + adapterFee;
    }
    
    /**
     * @dev Ước tính phí thấp nhất cho chain đích
     * @param dstChainId ID của chain đích
     * @param amount Số lượng token gửi
     * @return Tổng phí thấp nhất và adapter tương ứng
     */
    function estimateLowestFee(uint16 dstChainId, uint256 amount) 
        external
        view
        returns (uint256 fee, address adapter)
    {
        require(bridgeAdapters[dstChainId].length > 0, "No adapter for destination chain");
        
        uint256 protocolFee = calculateFee(amount);
        uint256 amountAfterFee = amount - protocolFee;
        uint256 lowestFee = type(uint256).max;
        address bestAdapter = address(0);
        
        for (uint i = 0; i < bridgeAdapters[dstChainId].length; i++) {
            address currentAdapter = bridgeAdapters[dstChainId][i];
            uint256 adapterFee = IBridgeAdapter(currentAdapter).estimateFee(dstChainId, amountAfterFee);
            
            if (adapterFee < lowestFee) {
                lowestFee = adapterFee;
                bestAdapter = currentAdapter;
            }
        }
        
        return (protocolFee + lowestFee, bestAdapter);
    }
    
    /**
     * @dev Cập nhật tỷ lệ phí
     * @param _feePercentage Tỷ lệ phí mới (0.1% = 1, 1% = 10)
     */
    function updateFeePercentage(uint256 _feePercentage) external onlyOwner {
        require(_feePercentage <= MAX_FEE, "Fee too high");
        feePercentage = _feePercentage;
        emit FeeUpdated(_feePercentage);
    }
    
    /**
     * @dev Cập nhật địa chỉ thu phí
     * @param _feeCollector Địa chỉ thu phí mới
     */
    function updateFeeCollector(address _feeCollector) external onlyOwner {
        require(_feeCollector != address(0), "Invalid address");
        feeCollector = _feeCollector;
        emit FeeCollectorUpdated(_feeCollector);
    }
    
    /**
     * @dev Tạm dừng bridge interface
     */
    function pause() external onlyOwner {
        _pause();
    }
    
    /**
     * @dev Tiếp tục bridge interface
     */
    function unpause() external onlyOwner {
        _unpause();
    }
    
    /**
     * @dev Lấy địa chỉ của WrappedDMD token
     * @return Địa chỉ WrappedDMD token
     */
    function getTokenAddress() public view virtual returns (address) {
        // Sẽ được override bởi các contract thừa kế
        return address(0);
    }
    
    /**
     * @dev Rút ETH khỏi contract (cho trường hợp khẩn cấp)
     * @param amount Số lượng ETH cần rút
     */
    function withdrawETH(uint256 amount) external onlyOwner {
        require(amount <= address(this).balance, "Insufficient balance");
        payable(owner()).transfer(amount);
    }
    
    /**
     * @dev Rút token khỏi contract (cho trường hợp khẩn cấp)
     * @param token Địa chỉ token cần rút
     * @param amount Số lượng token cần rút
     */
    function withdrawToken(address token, uint256 amount) external onlyOwner {
        IERC20 tokenContract = IERC20(token);
        require(amount <= tokenContract.balanceOf(address(this)), "Insufficient balance");
        require(tokenContract.transfer(owner(), amount), "Transfer failed");
    }
}

/**
 * @title DmdBscBridge
 * @dev Implementation của BridgeInterface cho BSC
 */
contract DmdBscBridge is BridgeInterface {
    IWrappedDMD public wrappedToken;
    IDiamondToken public diamondToken;
    
    /**
     * @dev Khởi tạo DmdBscBridge
     * @param _wrappedToken Địa chỉ WrappedDMD token
     * @param _diamondToken Địa chỉ DMD token (ERC1155)
     * @param _feeCollector Địa chỉ thu phí
     */
    constructor(
        address _wrappedToken,
        address _diamondToken,
        address _feeCollector
    ) BridgeInterface(_feeCollector) {
        require(_wrappedToken != address(0), "Invalid wrapped token");
        require(_diamondToken != address(0), "Invalid diamond token");
        
        wrappedToken = IWrappedDMD(_wrappedToken);
        diamondToken = IDiamondToken(_diamondToken);
    }
    
    /**
     * @dev Override getTokenAddress để trả về địa chỉ WrappedDMD token
     * @return Địa chỉ WrappedDMD token
     */
    function getTokenAddress() public view virtual override returns (address) {
        return address(wrappedToken);
    }
}

/**
 * @title ERC20Bridge
 * @dev Implementation của BridgeInterface cho ERC20
 */
contract ERC20Bridge is BridgeInterface {
    IWrappedDMD public wrappedToken;
    
    /**
     * @dev Khởi tạo ERC20Bridge
     * @param _wrappedToken Địa chỉ WrappedDMD token
     * @param _feeCollector Địa chỉ thu phí
     */
    constructor(
        address _wrappedToken,
        address _feeCollector
    ) BridgeInterface(_feeCollector) {
        require(_wrappedToken != address(0), "Invalid wrapped token");
        wrappedToken = IWrappedDMD(_wrappedToken);
    }
    
    /**
     * @dev Override getTokenAddress để trả về địa chỉ WrappedDMD token
     * @return Địa chỉ WrappedDMD token
     */
    function getTokenAddress() public view virtual override returns (address) {
        return address(wrappedToken);
    }
} 