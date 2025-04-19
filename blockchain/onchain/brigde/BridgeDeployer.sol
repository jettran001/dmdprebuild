// SPDX-License-Identifier: MIT
pragma solidity 0.8.20;

import "./bridge_interface.sol";
import "./bridge_adapter/LayerZeroAdapter.sol";
import "./bridge_adapter/WormholeAdapter.sol";

/**
 * @title BridgeDeployer
 * @dev Contract triển khai BridgeInterface và các adapter, đăng ký chúng tự động
 */
contract BridgeDeployer {
    // Các biến lưu trữ địa chỉ đã triển khai
    BridgeInterface public bridgeInterface;
    address public layerZeroAdapter;
    address public wormholeAdapter;
    
    // Biến lưu trữ admin
    address public admin;
    
    // Sự kiện
    event BridgeInterfaceDeployed(address indexed bridgeInterface);
    event AdapterDeployed(string adapterType, address indexed adapter);
    event AdapterRegistered(string adapterType, address indexed adapter);
    event DeploymentCompleted(address indexed bridgeInterface, address indexed admin);
    
    /**
     * @dev Khởi tạo BridgeDeployer
     * @param _admin Địa chỉ admin sẽ được chuyển quyền sở hữu sau khi triển khai
     */
    constructor(address _admin) {
        require(_admin != address(0), "Invalid admin address");
        admin = _admin;
    }
    
    /**
     * @dev Triển khai BridgeInterface và các adapter
     * @param wrappedToken Địa chỉ của wrapped token
     * @param diamondToken Địa chỉ của diamond token (null cho ERC20Bridge)
     * @param feeCollector Địa chỉ thu phí
     * @param lzEndpoint Địa chỉ Layer Zero Endpoint
     * @param wormholeCore Địa chỉ Wormhole Core Contract
     * @param targetChainIds Danh sách các chain ID đích
     */
    function deployBridgeSystem(
        address wrappedToken, 
        address diamondToken, 
        address feeCollector,
        address lzEndpoint,
        address wormholeCore,
        uint16[] memory targetChainIds
    ) external {
        // Triển khai BridgeInterface
        if (diamondToken != address(0)) {
            // Triển khai DmdBscBridge nếu có diamondToken
            bridgeInterface = new DmdBscBridge(
                wrappedToken,
                diamondToken,
                feeCollector
            );
        } else {
            // Triển khai ERC20Bridge nếu không có diamondToken
            bridgeInterface = new ERC20Bridge(
                wrappedToken,
                feeCollector
            );
        }
        emit BridgeInterfaceDeployed(address(bridgeInterface));
        
        // Triển khai LayerZero Adapter
        LayerZeroAdapter lzAdapter = new LayerZeroAdapter(
            lzEndpoint,
            targetChainIds[0], // Layer Zero target chain ID
            targetChainIds
        );
        layerZeroAdapter = address(lzAdapter);
        emit AdapterDeployed("LayerZero", layerZeroAdapter);
        
        // Triển khai Wormhole Adapter
        WormholeAdapter wAdapter = new WormholeAdapter(
            wormholeCore,
            targetChainIds[0], // Wormhole target chain ID
            targetChainIds
        );
        wormholeAdapter = address(wAdapter);
        emit AdapterDeployed("Wormhole", wormholeAdapter);
        
        // Đăng ký các adapter vào BridgeInterface
        bridgeInterface.registerBridge(layerZeroAdapter);
        emit AdapterRegistered("LayerZero", layerZeroAdapter);
        
        bridgeInterface.registerBridge(wormholeAdapter);
        emit AdapterRegistered("Wormhole", wormholeAdapter);
        
        // Chuyển quyền sở hữu của BridgeInterface cho admin
        bridgeInterface.transferOwnership(admin);
        
        emit DeploymentCompleted(address(bridgeInterface), admin);
    }
    
    /**
     * @dev Lấy thông tin hệ thống đã triển khai
     * @return Địa chỉ BridgeInterface, LayerZero Adapter, Wormhole Adapter và Admin
     */
    function getDeployedSystem() external view returns (
        address _bridgeInterface,
        address _layerZeroAdapter,
        address _wormholeAdapter,
        address _admin
    ) {
        return (
            address(bridgeInterface),
            layerZeroAdapter,
            wormholeAdapter,
            admin
        );
    }
}

/**
 * @title BridgeHelper
 * @dev Cung cấp các hàm trợ giúp cho việc tương tác với hệ thống Bridge
 */
contract BridgeHelper {
    /**
     * @dev Lấy danh sách các adapter cho một chain
     * @param bridgeInterface Địa chỉ của BridgeInterface
     * @param chainId Chain ID cần kiểm tra
     * @return Danh sách các adapter hỗ trợ chain này
     */
    function getAdaptersForChain(address bridgeInterface, uint16 chainId) external view returns (address[] memory) {
        return BridgeInterface(bridgeInterface).getAdaptersForChain(chainId);
    }
    
    /**
     * @dev Ước tính phí bridge với adapter có phí thấp nhất
     * @param bridgeInterface Địa chỉ của BridgeInterface
     * @param chainId Chain ID đích
     * @param amount Số lượng token cần bridge
     * @return Phí thấp nhất và adapter tương ứng
     */
    function estimateLowestFee(address bridgeInterface, uint16 chainId, uint256 amount) external view returns (uint256 fee, address adapter) {
        return BridgeInterface(bridgeInterface).estimateLowestFee(chainId, amount);
    }
    
    /**
     * @dev Kiểm tra một chain có được hỗ trợ không
     * @param bridgeInterface Địa chỉ của BridgeInterface
     * @param chainId Chain ID cần kiểm tra
     * @return true nếu có ít nhất một adapter hỗ trợ chain này
     */
    function isChainSupported(address bridgeInterface, uint16 chainId) external view returns (bool) {
        address[] memory adapters = BridgeInterface(bridgeInterface).getAdaptersForChain(chainId);
        return adapters.length > 0;
    }
} 