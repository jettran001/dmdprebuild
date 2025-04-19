// SPDX-License-Identifier: MIT
pragma solidity 0.8.20;

import "./IBridgeAdapter.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/security/ReentrancyGuard.sol";

/**
 * @title IWormhole
 * @dev Interface cho Wormhole protocol
 */
interface IWormhole {
    function publishMessage(
        uint32 nonce,
        bytes memory payload,
        uint8 consistencyLevel
    ) external payable returns (uint64 sequence);
}

/**
 * @title WormholeAdapter
 * @dev Adapter kết nối với Wormhole protocol cho bridging
 */
contract WormholeAdapter is IBridgeAdapter, Ownable, ReentrancyGuard {
    // Constants
    string private constant ADAPTER_TYPE = "Wormhole";
    uint16 private immutable _targetChain;
    uint16[] private _supportedChains;
    
    // Wormhole contract
    address public wormhole;
    uint8 public consistencyLevel = 1; // finality level
    
    // Bridge limits
    uint256 public limitPerTx; // Giới hạn số lượng token trên mỗi giao dịch
    uint256 public limitPerPeriod; // Giới hạn số lượng token trong một khoảng thời gian
    uint256 public periodDuration = 1 days; // Thời gian của một khoảng thời gian (mặc định: 1 ngày)
    mapping(uint256 => uint256) public periodVolumes; // period => volume
    
    // Mapping chain id to chain name
    mapping(uint16 => string) public chainNames;
    
    // Events
    event MessageSent(uint64 sequence, bytes payload);
    event WormholeAddressUpdated(address indexed newAddress);
    event ConsistencyLevelUpdated(uint8 level);
    event LimitUpdated(uint256 limitPerTx, uint256 limitPerPeriod, uint256 periodDuration);
    event ChainNameSet(uint16 indexed chainId, string name);
    
    // Trusted remotes mapping
    mapping(uint16 => bytes) public trustedRemotes; // chainId => remote address
    
    /**
     * @dev Khởi tạo WormholeAdapter
     * @param _wormhole Địa chỉ contract Wormhole
     * @param targetChainId ID của chain đích
     * @param supportedChainIds Danh sách các chain được hỗ trợ
     * @param _limitPerTx Giới hạn số lượng token trên mỗi giao dịch
     * @param _limitPerPeriod Giới hạn số lượng token trong một khoảng thời gian
     */
    constructor(
        address _wormhole,
        uint16 targetChainId,
        uint16[] memory supportedChainIds,
        uint256 _limitPerTx,
        uint256 _limitPerPeriod
    ) {
        require(_wormhole != address(0), "Invalid Wormhole address");
        wormhole = _wormhole;
        _targetChain = targetChainId;
        _supportedChains = supportedChainIds;
        limitPerTx = _limitPerTx;
        limitPerPeriod = _limitPerPeriod;
        
        // Khởi tạo tên chain chuẩn
        _initializeChainNames();
    }
    
    /**
     * @dev Khởi tạo tên chain chuẩn
     */
    function _initializeChainNames() internal {
        chainNames[1] = "Ethereum";
        chainNames[56] = "BSC";
        chainNames[137] = "Polygon";
        chainNames[43114] = "Avalanche";
        chainNames[10] = "Optimism";
        chainNames[42161] = "Arbitrum";
        chainNames[250] = "Fantom";
        chainNames[1284] = "Moonbeam";
        chainNames[1285] = "Moonriver";
        // Thêm các chain khác theo cần thiết
    }
    
    /**
     * @dev Lấy khoảng thời gian hiện tại
     * @return Khoảng thời gian hiện tại
     */
    function _getCurrentPeriod() internal view returns (uint256) {
        return block.timestamp / periodDuration;
    }
    
    /**
     * @dev Kiểm tra và cập nhật khối lượng trong khoảng thời gian
     * @param amount Số lượng token cần bridge
     * @return true nếu có thể bridge
     */
    function _checkAndUpdatePeriodVolume(uint256 amount) internal returns (bool) {
        uint256 currentPeriod = _getCurrentPeriod();
        uint256 newVolume = periodVolumes[currentPeriod] + amount;
        
        if (newVolume > limitPerPeriod) {
            return false;
        }
        
        periodVolumes[currentPeriod] = newVolume;
        return true;
    }
    
    /**
     * @dev Bridge token đến chain đích qua Wormhole
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
        require(trustedRemotes[dstChainId].length > 0, "Trusted remote not set for chain");
        require(amount > 0, "Amount must be greater than 0");
        require(amount <= limitPerTx, "Amount exceeds limitPerTx");
        require(destination.length > 0, "Empty destination");
        
        // Kiểm tra giới hạn số lượng token trong khoảng thời gian
        bool withinLimit = _checkAndUpdatePeriodVolume(amount);
        require(withinLimit, "Bridge volume exceeds limitPerPeriod");
        
        try this._executeBridge(dstChainId, destination, amount, msg.value, msg.sender) {
            // Bridge successful
        } catch Error(string memory reason) {
            // Log error và emit event
            emit BridgeFailed(msg.sender, dstChainId, destination, amount, reason);
            
            // Hoàn trả ETH
            (bool success, ) = msg.sender.call{value: msg.value}("");
            require(success, "ETH refund failed");
            
            revert(reason);
        } catch (bytes memory /*lowLevelData*/) {
            // Log error cho low-level failures
            emit BridgeFailed(msg.sender, dstChainId, destination, amount, "Low-level call failed");
            
            // Hoàn trả ETH
            (bool success, ) = msg.sender.call{value: msg.value}("");
            require(success, "ETH refund failed");
            
            revert("Bridge transaction failed");
        }
    }
    
    /**
     * @dev Thực hiện bridge (được gọi từ bridgeTo)
     * @param dstChainId ID của chain đích
     * @param destination Địa chỉ đích được mã hóa
     * @param amount Số lượng token cần bridge
     * @param msgValue Số lượng ETH gửi kèm
     * @param sender Địa chỉ người gửi
     */
    function _executeBridge(
        uint16 dstChainId,
        bytes calldata destination,
        uint256 amount,
        uint256 msgValue,
        address sender
    ) external {
        require(msg.sender == address(this), "Only callable from bridgeTo");
        
        // Đóng gói payload với thông tin cần thiết
        bytes memory payload = abi.encode(destination, amount, sender);
        
        // Gửi qua Wormhole
        uint64 sequence = IWormhole(wormhole).publishMessage{value: msgValue}(
            uint32(block.timestamp), // nonce
            payload,
            consistencyLevel
        );
        
        emit BridgeInitiated(sender, dstChainId, destination, amount);
        emit MessageSent(sequence, payload);
    }
    
    /**
     * @dev Ước tính phí bridge (phí cố định cho Wormhole)
     * @param dstChainId ID của chain đích
     * @param amount Số lượng token cần bridge (không sử dụng trong tính phí)
     * @return Phí bridge ước tính
     */
    function estimateFee(uint16 dstChainId, uint256 amount) external view override returns (uint256) {
        require(isChainSupported(dstChainId), "Chain not supported");
        
        // Wormhole có phí cố định cho mỗi message
        // Đây là giá trị mẫu, cần được cập nhật tùy thuộc vào cấu hình thực tế
        return 0.01 ether;
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
     * @return Tên loại adapter (Wormhole)
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
     * @dev Lấy tên của chain theo ID
     * @param chainId ID của chain cần lấy tên
     * @return Tên của chain
     */
    function getChainName(uint16 chainId) external view override returns (string memory) {
        string memory name = chainNames[chainId];
        require(bytes(name).length > 0, "Chain name not found");
        return name;
    }
    
    /**
     * @dev Cập nhật địa chỉ contract Wormhole
     * @param _wormhole Địa chỉ mới của contract Wormhole
     */
    function updateWormholeAddress(address _wormhole) external onlyOwner {
        require(_wormhole != address(0), "Invalid Wormhole address");
        wormhole = _wormhole;
        emit WormholeAddressUpdated(_wormhole);
    }
    
    /**
     * @dev Cập nhật mức độ consistency của Wormhole
     * @param _consistencyLevel Mức độ consistency mới
     */
    function updateConsistencyLevel(uint8 _consistencyLevel) external onlyOwner {
        consistencyLevel = _consistencyLevel;
        emit ConsistencyLevelUpdated(_consistencyLevel);
    }
    
    /**
     * @dev Cập nhật giới hạn bridge
     * @param _limitPerTx Giới hạn số lượng token trên mỗi giao dịch
     * @param _limitPerPeriod Giới hạn số lượng token trong một khoảng thời gian
     * @param _periodDuration Thời gian của một khoảng thời gian
     */
    function updateLimits(
        uint256 _limitPerTx,
        uint256 _limitPerPeriod,
        uint256 _periodDuration
    ) external onlyOwner {
        require(_limitPerTx > 0, "limitPerTx must be greater than 0");
        require(_limitPerPeriod > 0, "limitPerPeriod must be greater than 0");
        require(_periodDuration > 0, "periodDuration must be greater than 0");
        
        limitPerTx = _limitPerTx;
        limitPerPeriod = _limitPerPeriod;
        periodDuration = _periodDuration;
        
        emit LimitUpdated(_limitPerTx, _limitPerPeriod, _periodDuration);
    }
    
    /**
     * @dev Cập nhật trusted remote cho một chain
     * @param chainId ID của chain
     * @param remoteAddress Địa chỉ remote được tin cậy
     */
    function setTrustedRemote(uint16 chainId, bytes calldata remoteAddress) external onlyOwner {
        require(isChainSupported(chainId), "Chain not supported");
        require(remoteAddress.length > 0, "Empty remote address");
        trustedRemotes[chainId] = remoteAddress;
    }
    
    /**
     * @dev Lấy trusted remote cho một chain
     * @param chainId ID của chain
     * @return Địa chỉ remote được tin cậy
     */
    function getTrustedRemote(uint16 chainId) external view returns (bytes memory) {
        return trustedRemotes[chainId];
    }
    
    /**
     * @dev Cập nhật tên chain
     * @param chainId ID của chain
     * @param name Tên chain
     */
    function setChainName(uint16 chainId, string calldata name) external onlyOwner {
        require(isChainSupported(chainId), "Chain not supported");
        require(bytes(name).length > 0, "Empty name");
        
        chainNames[chainId] = name;
        
        emit ChainNameSet(chainId, name);
    }
    
    /**
     * @dev Rút token bị kẹt trong trường hợp khẩn cấp
     * @param to Địa chỉ nhận token
     */
    function emergencyWithdraw(address payable to) external onlyOwner {
        require(to != address(0), "Invalid recipient");
        
        uint256 balance = address(this).balance;
        require(balance > 0, "No ETH to withdraw");
        
        (bool success, ) = to.call{value: balance}("");
        require(success, "ETH transfer failed");
    }
    
    /**
     * @dev Nhận ETH
     */
    receive() external payable {}
}
