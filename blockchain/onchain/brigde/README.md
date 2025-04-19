# Bridge Module cho Diamond Chain

## Cấu trúc thư mục

```
brigde/
├── bridge_interface.sol         # Router trung tâm, quản lý các adapter và định tuyến giao dịch
├── bridge_adapter/              # Thư mục chứa các adapter cho các bridge protocol
│   ├── IBridgeAdapter.sol       # Interface chuẩn cho tất cả các adapter
│   ├── LayerZeroAdapter.sol     # Adapter cho LayerZero protocol
│   └── WormholeAdapter.sol      # Adapter cho Wormhole protocol
├── BridgeDeployer.sol           # Factory contract để triển khai và liên kết các module
├── erc20_wrappeddmd.sol         # ERC-20 đại diện trong quá trình bridge
├── erc1155_wrapper.sol          # Wrapper cho ERC-1155 thành ERC-20
├── erc1155_bridge_adapter.sol   # Adapter kết nối với bridge protocol
└── erc1155_unwrapper_near.rs    # Bộ giải nén trên NEAR
```

## Mô tả các thành phần

### 1. BridgeInterface.sol
- **Mô tả**: Router đa bridge quản lý các bridge adapter và điều hướng giao dịch qua adapter phù hợp.
- **Tính năng chính**:
  - Quản lý danh sách các adapter đã đăng ký
  - Cho phép chọn adapter (LayerZero, Wormhole, Custom...)
  - Điều hướng các yêu cầu bridge đến adapter thích hợp
  - Tính toán và quản lý phí bridge
  - Tự động chọn adapter với phí thấp nhất

### 2. Adapter (LayerZeroAdapter.sol, WormholeAdapter.sol)
- **Mô tả**: Các adapter kết nối với các bridge protocol cụ thể.
- **Tính năng chính**:
  - Triển khai interface IBridgeAdapter
  - Kết nối với các protocol cross-chain (LayerZero, Wormhole)
  - Ước tính phí bridge
  - Quản lý danh sách các chain được hỗ trợ

### 3. BridgeDeployer.sol
- **Mô tả**: Factory contract để triển khai và liên kết các thành phần bridge.
- **Tính năng chính**:
  - Triển khai BridgeInterface
  - Triển khai các adapter (LayerZero, Wormhole)
  - Đăng ký các adapter vào BridgeInterface
  - Chuyển quyền sở hữu của các contract cho admin

## Hướng dẫn triển khai

### Triển khai qua BridgeDeployer

1. **Compile các contract**:
   ```bash
   npx hardhat compile
   ```

2. **Triển khai BridgeDeployer**:
   ```javascript
   const BridgeDeployer = await ethers.getContractFactory("BridgeDeployer");
   const deployer = await BridgeDeployer.deploy(adminAddress);
   await deployer.deployed();
   console.log("BridgeDeployer deployed to:", deployer.address);
   ```

3. **Triển khai toàn bộ hệ thống bridge**:
   ```javascript
   const tx = await deployer.deployBridgeSystem(
     wrappedTokenAddress,    // Địa chỉ của wrapped token (ERC-20)
     diamondTokenAddress,    // Địa chỉ của diamond token (ERC-1155), hoặc address(0) cho ERC20Bridge
     feeCollectorAddress,    // Địa chỉ thu phí
     lzEndpointAddress,      // Địa chỉ LayerZero Endpoint
     wormholeCoreAddress,    // Địa chỉ Wormhole Core
     targetChainIds          // Danh sách các chain ID đích, ví dụ: [10, 56, 137]
   );
   await tx.wait();
   
   // Lấy thông tin hệ thống đã triển khai
   const deployedSystem = await deployer.getDeployedSystem();
   console.log("Bridge Interface:", deployedSystem._bridgeInterface);
   console.log("LayerZero Adapter:", deployedSystem._layerZeroAdapter);
   console.log("Wormhole Adapter:", deployedSystem._wormholeAdapter);
   ```

### Tương tác với hệ thống Bridge

#### Sử dụng BridgeInterface để bridge token:

```javascript
// Khởi tạo BridgeInterface contract
const bridgeInterface = await ethers.getContractAt("BridgeInterface", deployedSystem._bridgeInterface);

// Ước tính phí bridge với adapter có phí thấp nhất
const { fee, adapter } = await bridgeInterface.estimateLowestFee(destinationChainId, amount);
console.log("Lowest fee:", ethers.utils.formatEther(fee), "ETH, adapter:", adapter);

// Bridge token tự động chọn adapter tốt nhất
const tx = await bridgeInterface.autoBridge(
  destinationChainId,
  destinationAddress,
  amount,
  { value: fee }
);
await tx.wait();
```

## Mở rộng hệ thống

Để thêm adapter mới, tạo contract mới trong thư mục `bridge_adapter/` tuân theo interface `IBridgeAdapter.sol`, sau đó đăng ký nó vào BridgeInterface:

```javascript
// Triển khai adapter mới
const NewAdapter = await ethers.getContractFactory("NewAdapter");
const newAdapter = await NewAdapter.deploy(params);
await newAdapter.deployed();

// Đăng ký adapter mới vào BridgeInterface
await bridgeInterface.registerBridge(newAdapter.address);
``` 