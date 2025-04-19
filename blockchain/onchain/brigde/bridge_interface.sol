// SPDX-License-Identifier: MIT
pragma solidity 0.8.20;

import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/security/Pausable.sol";
import "@openzeppelin/contracts/utils/Counters.sol";
import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "./bridge_adapter/IBridgeAdapter.sol";

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
    event CustomBridgeAdapterAdded(string adapterName, address indexed adapter, uint256 adapterId);
    event AdapterConfigUpdated(address indexed adapter, bytes newConfig);
    event ChainSupportAdded(address indexed adapter, uint16 chainId);
    
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
    
    // Token address
    address private wrappedDMDToken;
    
    /**
     * @dev Thêm timelocks cho quyền rút tiền
     */
    struct Withdrawal {
        uint256 amount;
        uint256 unlockTime;
        bool approved;
        address[] approvers;
        mapping(address => bool) hasApproved;
    }

    uint256 private constant TIMELOCK_DURATION = 2 days;
    uint256 private constant MIN_APPROVALS = 2;
    address[] public admins;
    mapping(bytes32 => Withdrawal) public pendingWithdrawals;
    
    /**
     * @dev Sự kiện cho các thao tác multi-sig
     */
    event WithdrawalRequested(bytes32 withdrawalId, address token, address recipient, uint256 amount, uint256 unlockTime);
    event WithdrawalApproved(bytes32 withdrawalId, address approver);
    event WithdrawalExecuted(bytes32 withdrawalId, address recipient, uint256 amount);
    event WithdrawalCancelled(bytes32 withdrawalId);
    
    modifier onlyAdmin() {
        bool isAdmin = false;
        for (uint i = 0; i < admins.length; i++) {
            if (msg.sender == admins[i]) {
                isAdmin = true;
                break;
            }
        }
        require(isAdmin, "Caller is not admin");
        _;
    }
    
    /**
     * @dev Khởi tạo BridgeInterface
     * @param _feeCollector Địa chỉ thu phí
     * @param _wrappedDMDToken Địa chỉ token WrappedDMD
     */
    constructor(address _feeCollector, address _wrappedDMDToken) {
        require(_feeCollector != address(0), "Fee collector cannot be zero address");
        require(_wrappedDMDToken != address(0), "WrappedDMD token cannot be zero address");
        feeCollector = _feeCollector;
        wrappedDMDToken = _wrappedDMDToken;
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
        if (fee > 0) {
            require(feeCollector != address(0), "Fee collector not set");
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
     * @dev Thêm một adapter tùy chỉnh mới vào hệ thống bridge
     * @param adapterName Tên của adapter mới (ví dụ: "Axelar", "Multichain")
     * @param adapterImplementation Địa chỉ của implementation adapter
     * @param adapterConfig Cấu hình tùy chỉnh cho adapter (JSON encoded)
     * @return ID của adapter mới được thêm vào
     */
    function addCustomBridgeAdapter(
        string calldata adapterName,
        address adapterImplementation,
        bytes calldata adapterConfig
    ) 
        external 
        onlyOwner 
        returns (uint256) 
    {
        require(adapterImplementation != address(0), "Invalid adapter implementation");
        require(bytes(adapterName).length > 0, "Adapter name cannot be empty");
        
        // Kiểm tra xem adapter có tuân thủ interface IBridgeAdapter không
        // bằng cách gọi một hàm trong interface và kiểm tra không revert
        try IBridgeAdapter(adapterImplementation).adapterType() returns (string memory) {
            // Nếu không revert, tiếp tục thêm adapter
        } catch {
            revert("Implementation does not conform to IBridgeAdapter");
        }
        
        // Thực hiện đăng ký adapter
        registerBridge(adapterImplementation);
        
        // Lưu thêm thông tin tùy chỉnh nếu cần (có thể mở rộng thêm)
        // Ví dụ: lưu cấu hình cho adapter nếu cần
        
        // Phát sự kiện tùy chỉnh cho adapter mới
        emit CustomBridgeAdapterAdded(adapterName, adapterImplementation, bridgeCounter.current());
        
        return bridgeCounter.current();
    }
    
    /**
     * @dev Cập nhật cấu hình cho adapter tùy chỉnh
     * @param adapter Địa chỉ của adapter cần cập nhật
     * @param newConfig Cấu hình mới (JSON encoded)
     */
    function updateAdapterConfig(address adapter, bytes calldata newConfig) external onlyOwner {
        require(registeredBridges[adapter], "Adapter not registered");
        
        // Lưu cấu hình mới nếu cần
        // Ví dụ: có thể lưu cấu hình vào một mapping
        
        emit AdapterConfigUpdated(adapter, newConfig);
    }
    
    /**
     * @dev Hỗ trợ một chain mới cho adapter hiện có
     * @param adapter Địa chỉ của adapter cần mở rộng
     * @param newChainId Chain ID mới để hỗ trợ
     */
    function addSupportedChain(address adapter, uint16 newChainId) external onlyOwner {
        require(registeredBridges[adapter], "Adapter not registered");
        
        // Thêm chain vào bridgeAdapters
        bridgeAdapters[newChainId].push(adapter);
        
        emit ChainSupportAdded(adapter, newChainId);
    }
    
    /**
     * @dev Lấy địa chỉ của token được sử dụng cho bridge
     * @return Địa chỉ token WrappedDMD
     */
    function getTokenAddress() public view returns (address) {
        return wrappedDMDToken;
    }
    
    /**
     * @dev Cập nhật địa chỉ token
     * @param _token Địa chỉ token mới
     */
    function updateTokenAddress(address _token) external onlyOwner {
        require(_token != address(0), "Token address cannot be zero");
        wrappedDMDToken = _token;
    }
    
    /**
     * @dev Thêm admin mới
     * @param admin Địa chỉ admin mới
     */
    function addAdmin(address admin) external onlyOwner {
        require(admin != address(0), "Admin cannot be zero address");
        for (uint i = 0; i < admins.length; i++) {
            require(admins[i] != admin, "Admin already exists");
        }
        admins.push(admin);
    }
    
    /**
     * @dev Xóa admin
     * @param admin Địa chỉ admin cần xóa
     */
    function removeAdmin(address admin) external onlyOwner {
        require(admins.length > MIN_APPROVALS, "Cannot remove admin: minimum required");
        
        uint indexToRemove = admins.length;
        for (uint i = 0; i < admins.length; i++) {
            if (admins[i] == admin) {
                indexToRemove = i;
                break;
            }
        }
        
        require(indexToRemove < admins.length, "Admin not found");
        
        // Thay thế admin cần xóa bằng admin cuối cùng và xóa admin cuối
        admins[indexToRemove] = admins[admins.length - 1];
        admins.pop();
    }
    
    /**
     * @dev Yêu cầu rút ETH
     * @param recipient Địa chỉ nhận ETH
     * @param amount Số lượng ETH cần rút
     * @return withdrawalId ID của yêu cầu rút tiền
     */
    function requestWithdrawETH(address payable recipient, uint256 amount) external onlyAdmin returns (bytes32) {
        require(recipient != address(0), "Invalid recipient");
        require(amount > 0, "Amount must be greater than 0");
        require(address(this).balance >= amount, "Insufficient balance");
        
        bytes32 withdrawalId = keccak256(abi.encodePacked("ETH", recipient, amount, block.timestamp));
        Withdrawal storage withdrawal = pendingWithdrawals[withdrawalId];
        
        withdrawal.amount = amount;
        withdrawal.unlockTime = block.timestamp + TIMELOCK_DURATION;
        withdrawal.approved = false;
        withdrawal.approvers = new address[](0);
        withdrawal.hasApproved[msg.sender] = true;
        withdrawal.approvers.push(msg.sender);
        
        emit WithdrawalRequested(withdrawalId, address(0), recipient, amount, withdrawal.unlockTime);
        
        return withdrawalId;
    }
    
    /**
     * @dev Yêu cầu rút token
     * @param token Địa chỉ của token
     * @param recipient Địa chỉ nhận token
     * @param amount Số lượng token cần rút
     * @return withdrawalId ID của yêu cầu rút tiền
     */
    function requestWithdrawToken(address token, address recipient, uint256 amount) external onlyAdmin returns (bytes32) {
        require(token != address(0), "Invalid token");
        require(recipient != address(0), "Invalid recipient");
        require(amount > 0, "Amount must be greater than 0");
        require(IERC20(token).balanceOf(address(this)) >= amount, "Insufficient balance");
        
        bytes32 withdrawalId = keccak256(abi.encodePacked(token, recipient, amount, block.timestamp));
        Withdrawal storage withdrawal = pendingWithdrawals[withdrawalId];
        
        withdrawal.amount = amount;
        withdrawal.unlockTime = block.timestamp + TIMELOCK_DURATION;
        withdrawal.approved = false;
        withdrawal.approvers = new address[](0);
        withdrawal.hasApproved[msg.sender] = true;
        withdrawal.approvers.push(msg.sender);
        
        emit WithdrawalRequested(withdrawalId, token, recipient, amount, withdrawal.unlockTime);
        
        return withdrawalId;
    }
    
    /**
     * @dev Phê duyệt rút tiền
     * @param withdrawalId ID của yêu cầu rút tiền
     */
    function approveWithdrawal(bytes32 withdrawalId) external onlyAdmin {
        Withdrawal storage withdrawal = pendingWithdrawals[withdrawalId];
        require(withdrawal.amount > 0, "Withdrawal not found");
        require(!withdrawal.approved, "Withdrawal already approved");
        require(!withdrawal.hasApproved[msg.sender], "Already approved");
        
        withdrawal.hasApproved[msg.sender] = true;
        withdrawal.approvers.push(msg.sender);
        
        if (withdrawal.approvers.length >= MIN_APPROVALS) {
            withdrawal.approved = true;
        }
        
        emit WithdrawalApproved(withdrawalId, msg.sender);
    }
    
    /**
     * @dev Hủy yêu cầu rút tiền
     * @param withdrawalId ID của yêu cầu rút tiền
     */
    function cancelWithdrawal(bytes32 withdrawalId) external onlyAdmin {
        Withdrawal storage withdrawal = pendingWithdrawals[withdrawalId];
        require(withdrawal.amount > 0, "Withdrawal not found");
        require(!withdrawal.approved || block.timestamp < withdrawal.unlockTime, "Cannot cancel: already approved and unlocked");
        
        delete pendingWithdrawals[withdrawalId];
        
        emit WithdrawalCancelled(withdrawalId);
    }
    
    /**
     * @dev Rút ETH
     * @param withdrawalId ID của yêu cầu rút tiền
     * @param recipient Địa chỉ nhận ETH
     */
    function withdrawETH(bytes32 withdrawalId, address payable recipient) external onlyAdmin {
        Withdrawal storage withdrawal = pendingWithdrawals[withdrawalId];
        require(withdrawal.amount > 0, "Withdrawal not found");
        require(withdrawal.approved, "Withdrawal not approved");
        require(block.timestamp >= withdrawal.unlockTime, "Timelock not expired");
        require(address(this).balance >= withdrawal.amount, "Insufficient balance");
        
        uint256 amount = withdrawal.amount;
        delete pendingWithdrawals[withdrawalId];
        
        (bool success, ) = recipient.call{value: amount}("");
        require(success, "Transfer failed");
        
        emit WithdrawalExecuted(withdrawalId, recipient, amount);
    }
    
    /**
     * @dev Rút token
     * @param withdrawalId ID của yêu cầu rút tiền
     * @param token Địa chỉ của token
     * @param recipient Địa chỉ nhận token
     */
    function withdrawToken(bytes32 withdrawalId, address token, address recipient) external onlyAdmin {
        Withdrawal storage withdrawal = pendingWithdrawals[withdrawalId];
        require(withdrawal.amount > 0, "Withdrawal not found");
        require(withdrawal.approved, "Withdrawal not approved");
        require(block.timestamp >= withdrawal.unlockTime, "Timelock not expired");
        require(IERC20(token).balanceOf(address(this)) >= withdrawal.amount, "Insufficient balance");
        
        uint256 amount = withdrawal.amount;
        delete pendingWithdrawals[withdrawalId];
        
        require(IERC20(token).transfer(recipient, amount), "Transfer failed");
        
        emit WithdrawalExecuted(withdrawalId, recipient, amount);
    }
    
    /**
     * @dev Function để nhận ETH
     */
    receive() external payable {
        // Nhận ETH
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
    ) BridgeInterface(_feeCollector, _wrappedToken) {
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
    ) BridgeInterface(_feeCollector, _wrappedToken) {
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