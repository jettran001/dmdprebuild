// SPDX-License-Identifier: MIT
pragma solidity 0.8.20;

import "./IBridgeAdapter.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/security/ReentrancyGuard.sol";

/**
 * @title CustomAdapterTemplate
 * @dev Template cho việc tạo một adapter tùy chỉnh kết nối với bridge protocol
 * @notice Bridge protocol triển khai adapter, có thể là Axelar, Poly Network, Synapse, Multichain, etc.
 */
contract CustomAdapterTemplate is IBridgeAdapter, Ownable, ReentrancyGuard {
    // Constants
    string private constant ADAPTER_TYPE = "CustomProtocol";
    uint16 private immutable _targetChain;
    uint16[] private _supportedChains;

    // Thông tin Chain
    mapping(uint16 => string) public chainNames; // chainId => tên chain
    
    // Cài đặt phí
    uint256 public baseFee; // Phí cơ bản
    uint256 public feePercentage; // Phí theo % (1 = 0.1%)
    uint256 public constant MAX_FEE_PERCENTAGE = 50; // Tối đa 5%
    address public feeCollector;
    
    // Quản lý quyền
    mapping(address => bool) public operators; // Địa chỉ có quyền quản trị adapter
    uint256 public constant TIMELOCK_DURATION = 1 days; // Thời gian chờ trước khi áp dụng thay đổi
    mapping(bytes32 => uint256) public pendingChanges; // actionId => unlockTime
    
    // Events
    event MessageSent(uint16 indexed chainId, bytes destination, uint256 amount);
    event FeeUpdated(uint256 baseFee, uint256 feePercentage);
    event FeeCollectorUpdated(address indexed newCollector);
    event OperatorAdded(address indexed operator);
    event OperatorRemoved(address indexed operator);
    event ChainNameSet(uint16 indexed chainId, string name);
    event ConfigChangeRequested(bytes32 indexed actionId, uint256 unlockTime);
    event ConfigChangeApplied(bytes32 indexed actionId);
    event ConfigChangeCancelled(bytes32 indexed actionId);
    
    /**
     * @dev Modifier cho phép chỉ owner hoặc operator thực hiện hành động
     */
    modifier onlyOperator() {
        require(owner() == msg.sender || operators[msg.sender], "Not operator");
        _;
    }
    
    /**
     * @dev Khởi tạo CustomAdapterTemplate
     * @param targetChainId ID của chain đích
     * @param supportedChainIds Danh sách các chain được hỗ trợ
     * @param _baseFee Phí cơ bản
     * @param _feePercentage Phí theo % (1 = 0.1%)
     * @param _feeCollector Địa chỉ nhận phí
     */
    constructor(
        uint16 targetChainId,
        uint16[] memory supportedChainIds,
        uint256 _baseFee,
        uint256 _feePercentage,
        address _feeCollector
    ) {
        require(_feePercentage <= MAX_FEE_PERCENTAGE, "Fee too high");
        require(_feeCollector != address(0), "Invalid fee collector");
        
        _targetChain = targetChainId;
        _supportedChains = supportedChainIds;
        baseFee = _baseFee;
        feePercentage = _feePercentage;
        feeCollector = _feeCollector;
        
        // Khởi tạo tên chain chuẩn
        _initializeChainNames();
    }
    
    /**
     * @dev Khởi tạo tên chain chuẩn
     * Lưu ý: Cần cập nhật theo từng protocol
     */
    function _initializeChainNames() internal {
        chainNames[1] = "Ethereum";
        chainNames[56] = "BSC";
        chainNames[137] = "Polygon";
        chainNames[43114] = "Avalanche";
        chainNames[10] = "Optimism";
        chainNames[42161] = "Arbitrum";
        chainNames[250] = "Fantom";
        // Thêm các chain khác theo cần thiết
    }
    
    /**
     * @dev Bridge token đến chain đích
     * @param dstChainId ID của chain đích
     * @param destination Địa chỉ đích được mã hóa
     * @param amount Số lượng token cần bridge
     */
    function bridgeTo(
        uint16 dstChainId,
        bytes calldata destination,
        uint256 amount
    ) external payable override nonReentrant {
        require(isChainSupported(dstChainId), "Chain not supported");
        require(destination.length > 0, "Empty destination");
        
        // Tính phí và kiểm tra
        uint256 fee = estimateFee(dstChainId, amount);
        require(msg.value >= fee, "Insufficient fee");
        
        // Logic gửi qua bridge - cần implement theo protocol cụ thể
        // ...
        
        // Gửi phí cho collector
        if (fee > 0 && feeCollector != address(0)) {
            (bool success, ) = feeCollector.call{value: fee}("");
            require(success, "Fee transfer failed");
        }
        
        // Hoàn trả phí dư thừa
        if (msg.value > fee) {
            (bool success, ) = msg.sender.call{value: msg.value - fee}("");
            require(success, "Refund failed");
        }
        
        emit BridgeInitiated(msg.sender, dstChainId, destination, amount);
        emit MessageSent(dstChainId, destination, amount);
    }
    
    /**
     * @dev Ước tính phí bridge
     * @param dstChainId ID của chain đích
     * @param amount Số lượng token cần bridge
     * @return Phí bridge ước tính
     */
    function estimateFee(uint16 dstChainId, uint256 amount) public view override returns (uint256) {
        require(isChainSupported(dstChainId), "Chain not supported");
        
        // Tính phí theo phần trăm + phí cơ bản
        uint256 percentFee = (amount * feePercentage) / 1000;
        return baseFee + percentFee;
    }
    
    /**
     * @dev Trả về danh sách các chain được hỗ trợ
     * @return Danh sách ID của các chain được hỗ trợ
     */
    function supportedChains() external view override returns (uint16[] memory) {
        return _supportedChains;
    }
    
    /**
     * @dev Trả về loại adapter
     * @return Tên loại adapter
     */
    function adapterType() external pure override returns (string memory) {
        return ADAPTER_TYPE;
    }
    
    /**
     * @dev Trả về ID của chain đích mà adapter này hỗ trợ
     * @return ID của chain đích
     */
    function targetChain() external view override returns (uint16) {
        return _targetChain;
    }
    
    /**
     * @dev Kiểm tra xem chain có được hỗ trợ không
     * @param chainId ID của chain cần kiểm tra
     * @return true nếu chain được hỗ trợ
     */
    function isChainSupported(uint16 chainId) public view override returns (bool) {
        for (uint256 i = 0; i < _supportedChains.length; i++) {
            if (_supportedChains[i] == chainId) {
                return true;
            }
        }
        return false;
    }
    
    /**
     * @dev Cập nhật phí bridge (sử dụng timelock)
     * @param _baseFee Phí cơ bản mới
     * @param _feePercentage Phí theo % mới
     */
    function requestFeeUpdate(uint256 _baseFee, uint256 _feePercentage) external onlyOperator {
        require(_feePercentage <= MAX_FEE_PERCENTAGE, "Fee too high");
        
        bytes32 actionId = keccak256(abi.encodePacked("FEE_UPDATE", _baseFee, _feePercentage, block.timestamp));
        pendingChanges[actionId] = block.timestamp + TIMELOCK_DURATION;
        
        emit ConfigChangeRequested(actionId, pendingChanges[actionId]);
    }
    
    /**
     * @dev Áp dụng cập nhật phí sau thời gian timelock
     * @param actionId ID của hành động
     * @param _baseFee Phí cơ bản mới
     * @param _feePercentage Phí theo % mới
     */
    function applyFeeUpdate(bytes32 actionId, uint256 _baseFee, uint256 _feePercentage) external onlyOperator {
        require(pendingChanges[actionId] > 0, "No pending change");
        require(block.timestamp >= pendingChanges[actionId], "Timelock not expired");
        require(_feePercentage <= MAX_FEE_PERCENTAGE, "Fee too high");
        
        delete pendingChanges[actionId];
        baseFee = _baseFee;
        feePercentage = _feePercentage;
        
        emit FeeUpdated(_baseFee, _feePercentage);
        emit ConfigChangeApplied(actionId);
    }
    
    /**
     * @dev Cập nhật địa chỉ nhận phí (sử dụng timelock)
     * @param _feeCollector Địa chỉ nhận phí mới
     */
    function requestFeeCollectorUpdate(address _feeCollector) external onlyOperator {
        require(_feeCollector != address(0), "Invalid fee collector");
        
        bytes32 actionId = keccak256(abi.encodePacked("FEE_COLLECTOR_UPDATE", _feeCollector, block.timestamp));
        pendingChanges[actionId] = block.timestamp + TIMELOCK_DURATION;
        
        emit ConfigChangeRequested(actionId, pendingChanges[actionId]);
    }
    
    /**
     * @dev Áp dụng cập nhật địa chỉ nhận phí sau thời gian timelock
     * @param actionId ID của hành động
     * @param _feeCollector Địa chỉ nhận phí mới
     */
    function applyFeeCollectorUpdate(bytes32 actionId, address _feeCollector) external onlyOperator {
        require(pendingChanges[actionId] > 0, "No pending change");
        require(block.timestamp >= pendingChanges[actionId], "Timelock not expired");
        require(_feeCollector != address(0), "Invalid fee collector");
        
        delete pendingChanges[actionId];
        feeCollector = _feeCollector;
        
        emit FeeCollectorUpdated(_feeCollector);
        emit ConfigChangeApplied(actionId);
    }
    
    /**
     * @dev Hủy bỏ một thay đổi cấu hình đang chờ
     * @param actionId ID của hành động
     */
    function cancelConfigChange(bytes32 actionId) external onlyOperator {
        require(pendingChanges[actionId] > 0, "No pending change");
        
        delete pendingChanges[actionId];
        
        emit ConfigChangeCancelled(actionId);
    }
    
    /**
     * @dev Thêm operator
     * @param operator Địa chỉ operator mới
     */
    function addOperator(address operator) external onlyOwner {
        require(operator != address(0), "Invalid operator");
        require(!operators[operator], "Already operator");
        
        operators[operator] = true;
        
        emit OperatorAdded(operator);
    }
    
    /**
     * @dev Xóa operator
     * @param operator Địa chỉ operator cần xóa
     */
    function removeOperator(address operator) external onlyOwner {
        require(operators[operator], "Not operator");
        
        operators[operator] = false;
        
        emit OperatorRemoved(operator);
    }
    
    /**
     * @dev Cập nhật tên chain
     * @param chainId ID của chain
     * @param name Tên chain
     */
    function setChainName(uint16 chainId, string calldata name) external onlyOperator {
        require(isChainSupported(chainId), "Chain not supported");
        require(bytes(name).length > 0, "Empty name");
        
        chainNames[chainId] = name;
        
        emit ChainNameSet(chainId, name);
    }
    
    /**
     * @dev Lấy tên chain
     * @param chainId ID của chain
     * @return Tên chain
     */
    function getChainName(uint16 chainId) external view returns (string memory) {
        return chainNames[chainId];
    }
} 