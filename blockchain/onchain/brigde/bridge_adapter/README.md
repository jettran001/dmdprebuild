# Bridge Adapter Mở Rộng

Thư mục này chứa các bridge adapter cho phép kết nối với các protocol bridge khác nhau như LayerZero, Wormhole, Axelar, v.v.

## Cách thêm Bridge Adapter mới

### 1. Sử dụng Template

Để tạo một bridge adapter mới, hãy sao chép và tùy chỉnh file `CustomAdapterTemplate.sol`:

```bash
# Sao chép template
cp CustomAdapterTemplate.sol NewBridgeAdapter.sol

# Chỉnh sửa adapter mới
vim NewBridgeAdapter.sol
```

### 2. Triển khai Adapter

Mỗi adapter mới cần phải triển khai interface `IBridgeAdapter.sol`. Các hàm chính cần triển khai:

- `bridgeTo`: Gửi token qua bridge đến chain đích
- `estimateFee`: Ước tính phí bridge
- `supportedChains`: Trả về danh sách các chain được hỗ trợ
- `adapterType`: Trả về loại của adapter
- `targetChain`: Trả về chain chính mà adapter này hỗ trợ
- `isChainSupported`: Kiểm tra một chain có được hỗ trợ không

### 3. Đăng ký Adapter vào Bridge Interface

Sau khi triển khai adapter, bạn cần đăng ký nó vào `BridgeInterface` bằng một trong hai cách:

#### Cách 1: Sử dụng hàm registerBridge trực tiếp

```solidity
// Triển khai adapter mới
NewBridgeAdapter adapter = new NewBridgeAdapter(...);

// Đăng ký adapter vào BridgeInterface
bridgeInterface.registerBridge(address(adapter));
```

#### Cách 2: Sử dụng hàm addCustomBridgeAdapter (Khuyến nghị)

```solidity
// Triển khai adapter mới
NewBridgeAdapter adapter = new NewBridgeAdapter(...);

// Đăng ký adapter với tên và cấu hình
bridgeInterface.addCustomBridgeAdapter(
    "NewProtocol",
    address(adapter),
    abi.encode(...) // Cấu hình tùy chỉnh nếu cần
);
```

### 4. Mở rộng cho Chain mới

Để thêm hỗ trợ cho một chain mới vào adapter hiện có:

```solidity
// Thêm chain mới vào adapter
adapter.addSupportedChain(NEW_CHAIN_ID);

// Đăng ký chain mới vào BridgeInterface
bridgeInterface.addSupportedChain(address(adapter), NEW_CHAIN_ID);
```

## Danh sách Chain ID

Dưới đây là danh sách ID các chain chính:

- Ethereum Mainnet: 1
- BSC (BNB Smart Chain): 56
- Polygon: 137
- Avalanche: 43114
- Arbitrum: 42161
- Optimism: 10
- Base: 8453
- NEAR: 1313161554
- Solana: 999999999 (ID tùy chỉnh, không phải tiêu chuẩn)

## Ví dụ Triển khai Adapter Mới

### Ví dụ: Adapter cho Axelar Network

```solidity
// SPDX-License-Identifier: MIT
pragma solidity 0.8.20;

import "./IBridgeAdapter.sol";
import "./IAxelarGateway.sol";

contract AxelarAdapter is IBridgeAdapter {
    address private axelarGateway;
    uint16 private targetChainId;
    uint16[] private supportedChainIds;
    address private owner;
    
    mapping(uint16 => string) private chainIdToName;
    
    constructor(
        address _axelarGateway,
        uint16 _targetChainId,
        uint16[] memory _supportedChainIds
    ) {
        axelarGateway = _axelarGateway;
        targetChainId = _targetChainId;
        owner = msg.sender;
        
        // Thiết lập các chain được hỗ trợ
        for (uint i = 0; i < _supportedChainIds.length; i++) {
            supportedChainIds.push(_supportedChainIds[i]);
        }
        
        // Thiết lập mapping từ chain ID sang tên chain của Axelar
        chainIdToName[1] = "ethereum";
        chainIdToName[56] = "binance";
        chainIdToName[137] = "polygon";
        // Thêm các chain khác nếu cần
    }
    
    function bridgeTo(uint16 dstChainId, bytes calldata destination, uint256 amount) external payable override {
        require(isChainSupported(dstChainId), "Chain not supported");
        
        // Giải mã địa chỉ đích từ destination
        address recipient = abi.decode(destination, (address));
        
        // Gọi Axelar Gateway để gửi token
        IAxelarGateway(axelarGateway).sendToken{value: msg.value}(
            chainIdToName[dstChainId],
            bytes32ToString(bytes32(uint256(recipient))),
            "aUSDC",
            amount
        );
        
        emit BridgeInitiated(msg.sender, dstChainId, destination, amount);
    }
    
    // Các hàm khác được triển khai theo template...
}
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