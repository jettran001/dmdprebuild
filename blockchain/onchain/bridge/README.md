# Bridge Module cho Diamond Chain

## Cấu trúc thư mục

```
brigde/
├── bridge_adapter/          # Chứa các adapter cho các bridge protocol
│   ├── IBridgeAdapter.sol     # Interface chuẩn cho tất cả các adapter
│   ├── LayerZeroAdapter.sol   # Adapter cho LayerZero protocol
│   └── WormholeAdapter.sol    # Adapter cho Wormhole protocol
├── bridge_interface.sol     # Router trung tâm, quản lý các adapter
├── BridgeDeployer.sol       # Factory liên kết các module
├── erc20_wrappeddmd.sol     # ERC-20 đại diện trong bridge
├── erc1155_wrapper.sol      # Wrapper cho ERC-1155 thành ERC-20
├── erc1155_bridge_adapter.sol # Adapter kết nối với bridge protocol
└── erc1155_unwrapper_near.rs  # Bộ giải nén trên NEAR
```

## Mối quan hệ giữa các thành phần

Mối quan hệ chi tiết giữa các file trong hệ thống:

```
blockchain/onchain/brigde/bridge_interface.sol ⟶ blockchain/onchain/brigde/bridge_adapter/IBridgeAdapter.sol
# Router gọi các bridge thông qua interface chuẩn

blockchain/onchain/brigde/bridge_adapter/IBridgeAdapter.sol ⟶ Interface giữa các adapter
# Giao diện chuẩn hóa cho tất cả các adapter

blockchain/onchain/brigde/bridge_adapter/LayerZeroAdapter.sol ⟵ implements blockchain/onchain/brigde/bridge_adapter/IBridgeAdapter.sol
# Thực thi bridge LayerZero

blockchain/onchain/brigde/bridge_adapter/WormholeAdapter.sol ⟵ implements blockchain/onchain/brigde/bridge_adapter/IBridgeAdapter.sol
# Thực thi bridge Wormhole
```

> **Quan trọng**: BridgeInterface không gọi trực tiếp adapter cụ thể. Nó chỉ gọi qua interface trung gian IBridgeAdapter. Adapter nào implement đúng chuẩn IBridgeAdapter đều có thể plug vào hệ thống ngay mà không cần thay đổi code của router.

## Luồng hoạt động của hệ thống

1. `BridgeInterface.sol` quản lý tất cả các bridge adapter đã đăng ký
2. Khi cần bridge token, `BridgeInterface.sol` sẽ:
   - Gọi `estimateFee()` trên `IBridgeAdapter` để ước tính phí
   - Gọi `bridgeTo()` trên `IBridgeAdapter` để thực hiện bridge
3. Mỗi adapter (LayerZero/Wormhole) thực hiện giao tiếp với protocol tương ứng
4. Dữ liệu bridge được chuyển qua protocol và xử lý ở chain đích
5. Toàn bộ quá trình hoạt động theo mô hình plugin, cho phép thêm adapter mới mà không cần sửa code của router

## Mô tả các thành phần

### 1. BridgeInterface.sol
- **Mô tả**: Router đa bridge quản lý các bridge adapter và điều hướng giao dịch qua adapter phù hợp.
- **Tính năng chính**:
  - Quản lý danh sách các adapter đã đăng ký
  - Không gọi trực tiếp adapter cụ thể mà luôn thông qua IBridgeAdapter
  - Cho phép chọn adapter (LayerZero, Wormhole, Custom...)
  - Điều hướng các yêu cầu bridge đến adapter thích hợp
  - Tính toán và quản lý phí bridge
  - Tự động chọn adapter với phí thấp nhất

### 2. IBridgeAdapter.sol
- **Mô tả**: Interface chuẩn hóa cho tất cả các adapter bridge.
- **Tính năng chính**:
  - Định nghĩa các phương thức chuẩn mà mọi adapter phải triển khai
  - Tạo lớp trừu tượng giữa BridgeInterface và các adapter cụ thể
  - Đảm bảo tính mở rộng của hệ thống
  - Cho phép thêm adapter mới mà không cần thay đổi BridgeInterface

### 3. Bridge Adapter (LayerZeroAdapter.sol, WormholeAdapter.sol)
- **Mô tả**: Các adapter cụ thể kết nối với các bridge protocol.
- **Tính năng chung**:
  - Triển khai interface IBridgeAdapter
  - Kết nối với các protocol cross-chain tương ứng
  - Ước tính phí bridge
  - Quản lý danh sách các chain được hỗ trợ
  - Xác minh chữ ký và các biện pháp bảo mật
  - Xử lý giới hạn bridge và rate limiting

### 4. BridgeDeployer.sol
- **Mô tả**: Factory contract để triển khai và liên kết các thành phần bridge.
- **Tính năng chính**:
  - Triển khai BridgeInterface
  - Triển khai các adapter (LayerZero, Wormhole)
  - Đăng ký các adapter vào BridgeInterface thông qua IBridgeAdapter
  - Chuyển quyền sở hữu của các contract cho admin

## Danh sách Chain ID

Dưới đây là danh sách ID các chain chính:

- Ethereum Mainnet: 1
- BSC (BNB Smart Chain): 56
- Polygon: 137
- Avalanche: 43114
- Arbitrum: 42161
- Optimism: 10
- Base: 8453
- Fantom: 250
- Moonbeam: 1284
- Moonriver: 1285
- NEAR: 1313161554
- Solana: 999999999 (ID tùy chỉnh, không phải tiêu chuẩn)

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

## Cách thêm Bridge Adapter mới

### 1. Tạo adapter mới theo chuẩn IBridgeAdapter

Để tạo một bridge adapter mới, bạn cần đảm bảo triển khai đúng interface IBridgeAdapter:

```solidity
// NewBridgeAdapter.sol
contract NewBridgeAdapter is IBridgeAdapter, Ownable, ReentrancyGuard {
    // Triển khai tất cả các phương thức từ IBridgeAdapter
    function bridgeTo(uint16 dstChainId, bytes calldata destination, uint256 amount) external payable override {
        // Triển khai cụ thể...
    }
    
    function estimateFee(uint16 dstChainId, uint256 amount) external view override returns (uint256) {
        // Triển khai cụ thể...
    }
    
    // Các phương thức khác...
}
```

### 2. Đăng ký Adapter vào Bridge Interface

Sau khi triển khai adapter, bạn cần đăng ký nó vào `BridgeInterface`:

```solidity
// Triển khai adapter mới
NewBridgeAdapter adapter = new NewBridgeAdapter(...);

// Đăng ký adapter vào BridgeInterface
bridgeInterface.registerBridge(address(adapter));
```

> **Lưu ý**: Chỉ cần adapter triển khai đúng IBridgeAdapter, BridgeInterface có thể tương tác với nó ngay mà không cần biết chi tiết cụ thể bên trong.

## Tương tác với hệ thống Bridge

### Sử dụng BridgeInterface để bridge token:

```javascript
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

## Các Protocol Bridge Phổ biến

- **LayerZero**: Cross-chain messaging protocol với tính bảo mật cao
- **Wormhole**: Cross-chain messaging protocol với khả năng mở rộng tốt
- **Axelar**: Messaging và liquidity network đa chuỗi
- **Multichain**: Cầu nối token đa chuỗi (trước đây là AnySwap)
- **cBridge**: Cầu nối thanh khoản của Celer Network

## Lưu ý Bảo mật

- Luôn kiểm tra kỹ các điều kiện bảo mật trong adapter mới
- Triển khai các cơ chế xác thực message thích hợp
- Kiểm tra kỹ lưỡng xem adapter có tuân thủ interface chuẩn hay không
- Xem xét việc giới hạn số lượng token có thể bridge trong một giao dịch 
- Sử dụng timelock và multisig cho các hành động nhạy cảm
- Triển khai các cơ chế rate-limiting để ngăn chặn tấn công 

## New Feature: wrap_to_chain Integration

In order to improve user experience and reduce gas costs, we've added a new function to the `erc20_wrappeddmd.sol` contract:

```solidity
function wrap_to_chain(
    uint256 amount, 
    uint16 dstChainId, 
    bytes calldata toAddress,
    bool needUnwrap
) external payable whenNotPaused nonReentrant returns (bytes32 bridgeId)
```

### Benefits:

- **Reduced gas costs**: Users can now mint and bridge in a single transaction
- **Improved security**: Tokens are automatically burned if the bridge fails
- **Better tracking**: Returns a unique bridgeId for monitoring the bridge operation
- **Automatic unwrapping**: Supports optional unwrapping at the destination chain

### Usage Example:

```solidity
// To bridge 1000 tokens to BSC chain (2) without auto-unwrap
bytes32 bridgeId = wrappedDMD.wrap_to_chain{value: 0.1 ether}(
    1000 ether,
    2, // BSC chain ID
    abi.encodePacked(receiverAddressOnBsc),
    false
);

// To bridge with auto-unwrap at destination:
bytes32 bridgeId = wrappedDMD.wrap_to_chain{value: 0.1 ether}(
    1000 ether,
    2, // BSC chain ID
    abi.encodePacked(receiverAddressOnBsc),
    true
);
```

### Security Features:

1. Uses nonReentrant guard to prevent reentrancy attacks
2. Verifies the bridge proxy is properly configured
3. Automatically burns tokens if the bridge operation fails
4. Emits detailed events for auditing and tracking

This addition brings the functionality in line with the `wrapAndBridge` and `wrapAndBridgeWithUnwrap` functions already present in the `erc1155_wrapper.sol` contract. 