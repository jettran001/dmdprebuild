# HƯỚNG DẪN TEST TOÀN DIỆN HỆ THỐNG BRIDGE

## Tổng quan

Tài liệu này cung cấp hướng dẫn chi tiết về việc test toàn diện hệ thống bridge cross-chain. Quy trình test được thiết kế để đảm bảo tính ổn định, bảo mật và hiệu suất của hệ thống bridge trước khi triển khai lên môi trường production.

## Cài đặt môi trường

### Yêu cầu

- Node.js v16.x trở lên
- Yarn hoặc npm
- Hardhat (`npm install --save-dev hardhat`)
- Foundry (`curl -L https://foundry.paradigm.xyz | bash`)
- LayerZero TestNet Endpoints (cho testnet của các chain)

### Cấu hình môi trường

1. Clone repository và cài đặt dependencies
```bash
git clone https://github.com/diamondchain/bridge
cd bridge
yarn install
```

2. Cấu hình Hardhat
```bash
cp .env.example .env
# Cập nhật các biến môi trường trong file .env
```

3. Cấu hình Foundry (nếu sử dụng)
```bash
forge install
```

## Cấu trúc test

### 1. Unit Tests

Các unit tests tập trung vào kiểm tra các thành phần độc lập trong hệ thống bridge.

#### Chạy unit tests

```bash
# Hardhat
npx hardhat test test/unit/

# Foundry
forge test --match-path "test/unit/*.sol"
```

#### Danh sách unit tests

- `BridgeInterface.test.js`: Kiểm tra các hàm của DiamondBridgeInterface
- `ERC20WrappedDMD.test.js`: Kiểm tra các hàm của ERC20 wrapped token
- `ERC1155Wrapper.test.js`: Kiểm tra các hàm wrap và unwrap
- `BridgePayloadCodec.test.js`: Kiểm tra encode/decode payload
- `ChainRegistry.test.js`: Kiểm tra registry và mapping

### 2. Integration Tests

Các integration tests kiểm tra tương tác giữa các thành phần trong hệ thống.

#### Chạy integration tests

```bash
# Hardhat
npx hardhat test test/integration/

# Foundry
forge test --match-path "test/integration/*.sol"
```

#### Danh sách integration tests

- `Bridge_ERC20.test.js`: Bridge ERC20 token giữa các chain
- `Bridge_ERC1155.test.js`: Wrap -> Bridge -> Unwrap ERC1155 tokens
- `Bridge_Native.test.js`: Bridge native tokens
- `Bridge_AutoUnwrap.test.js`: Test auto-unwrap tại chain đích
- `Bridge_Retry.test.js`: Test retry cho các giao dịch thất bại

### 3. Flow Tests

Flow tests kiểm tra toàn bộ luồng bridge từ đầu đến cuối.

#### Chạy flow tests

```bash
npx hardhat test test/flows/
```

#### Danh sách flow tests

- `Full_Bridge_Flow.test.js`: Toàn bộ luồng bridge từ A -> B -> A
- `Cross_Chain_Flow.test.js`: Kiểm tra bridge qua nhiều chain (A -> B -> C)
- `Recovery_Flow.test.js`: Kiểm tra luồng recovery và emergency
- `Multi_Transaction_Flow.test.js`: Kiểm tra nhiều bridge đồng thời

### 4. Test Môi trường đa chain

Test trên môi trường thực tế với nhiều testnet chains khác nhau.

#### Chuẩn bị

```bash
# Setup local fork của các testnet
npx hardhat node --fork https://eth-goerli.alchemyapi.io/v2/YOUR_API_KEY
```

#### Danh sách test cross-chain

- `Mainnet_Forking.test.js`: Test bằng cách fork mainnet
- `Testnet_Live.test.js`: Test trên các testnet thực tế

## Test Case Chi Tiết

### A. Bridge Token ERC20

1. **Setup**
   - Triển khai contracts bridge trên chain A và B
   - Setup trusted remote giữa các chain
   - Mint test tokens cho các tài khoản test

2. **Test Steps**
   - Gọi `bridgeToken` trên chain A với token ERC20
   - Verify token được lock/burn tại chain A
   - Verify LayerZero message được gửi đúng format
   - Verify token được mint tại chain B
   - Verify balances tại cả 2 chain
   - Verify các event được emit đúng

3. **Assertions**
   - Số lượng token nhận tại B = số lượng gửi từ A - phí
   - Token được lock/burn tại A
   - Event `BridgeInitiated` và `BridgeReceived` được emit với các thông số đúng

### B. Bridge Token ERC1155

1. **Setup**
   - Triển khai contracts bridge trên chain A và B
   - Triển khai wrapper và unwrapper
   - Setup trusted remote
   - Mint ERC1155 test tokens

2. **Test Steps**
   - Gọi `wrapAndBridgeWithUnwrap` trên chain A
   - Verify ERC1155 token được wrap thành ERC20
   - Verify ERC20 bị burn và message được gửi
   - Verify nhận và auto-unwrap tại chain B
   - Verify balances

3. **Assertions**
   - Số ERC1155 ở A giảm đúng số lượng gửi
   - Số ERC1155 ở B tăng đúng số lượng (trừ phí)
   - Events `TokenWrapped`, `BridgeInitiated`, `BridgeReceived`, `TokenUnwrapped` được emit

### C. Bridge Edge Cases

1. **Rate Limit Test**
   - Gửi lượng token lớn hơn `dailyLimit`
   - Verify transaction revert với error phù hợp

2. **Unauthorized Access Test**
   - Thử gọi các hàm admin với non-admin address
   - Verify revert với error phù hợp

3. **Invalid Input Test**
   - Bridge với amount = 0
   - Bridge với destination = empty bytes
   - Bridge với unsupported chain
   - Verify các error phù hợp

4. **Security Tests**
   - Thử replay attack (gửi lại message đã xử lý)
   - Thử giả mạo source chain/address
   - Verify các bảo vệ hoạt động đúng

### D. Recovery và Emergency Tests

1. **Stuck Token Recovery**
   - Gửi token trực tiếp vào contract (không qua bridge)
   - Sử dụng rescue functions để recover
   - Verify token được giải phóng thành công

2. **Emergency Withdrawal**
   - Setup emergency withdrawal request
   - Wait for timelock expiry
   - Execute withdrawal
   - Verify token được rút và event được emit

3. **Pause/Unpause Test**
   - Pause bridge
   - Thử bridge trong trạng thái paused
   - Unpause và bridge lại
   - Verify hoạt động đúng

## Performance Tests

### Gas Optimization

```bash
# Run gas reporter
REPORT_GAS=true npx hardhat test
```

### Load Tests

```bash
# Simulate multiple transactions
node scripts/loadtest.js --tx-count=100 --bridge-amount=1
```

## Coverage Tests

Đo lường độ phủ của test bằng:

```bash
# Hardhat coverage
npx hardhat coverage

# Foundry coverage
forge coverage
```

## Hướng dẫn test LayerZero integration

### 1. LayerZero Testnet Setup

1. Đăng ký endpoint testnet tại [LayerZero](https://layerzero.network/)
2. Cập nhật endpoint addresses trong `.env`
3. Fund các test accounts với testnet tokens

### 2. Test LayerZero integration

```bash
# Deploy trên testnet
npx hardhat run scripts/deploy-testnet.js --network goerli

# Test cross-chain message
npx hardhat run scripts/test-lz-bridge.js --network goerli
```

### 3. Monitor LayerZero messages

Sử dụng [LayerZero Scan](https://layerzeroscan.com/) để theo dõi các cross-chain messages trong quá trình test.

## Quy trình CI/CD

Thiết lập CI/CD pipeline để tự động test khi có thay đổi:

```yaml
# .github/workflows/test.yml
name: Test Bridge Contracts

on:
  push:
    branches: [ main, develop ]
  pull_request:
    branches: [ main, develop ]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - name: Setup Node.js
        uses: actions/setup-node@v3
        with:
          node-version: 16
      - name: Install dependencies
        run: yarn install
      - name: Run Hardhat tests
        run: npx hardhat test
      - name: Run Foundry tests
        run: |
          curl -L https://foundry.paradigm.xyz | bash
          foundryup
          forge test -vvv
```

## Lưu ý quan trọng

1. **Khởi tạo mạng test local**
   ```bash
   # Hardhat local network
   npx hardhat node
   
   # Anvil (Foundry)
   anvil
   ```

2. **Test cross-chain với mock LayerZero**
   ```bash
   # Deploy LayerZero mocks
   npx hardhat run scripts/deploy-lz-mock.js --network localhost
   ```

3. **Test trên testnet thực**
   ```bash
   # Deploy và test trên Goerli + Mumbai
   npx hardhat run scripts/cross-testnet-test.js
   ```

## Tài liệu tham khảo

- [Hardhat Documentation](https://hardhat.org/getting-started/)
- [Foundry Documentation](https://book.getfoundry.sh/)
- [LayerZero Documentation](https://layerzero.gitbook.io/docs/)
- [Solidity Coverage](https://github.com/sc-forks/solidity-coverage)

## Checklist test toàn diện

- [ ] Tất cả unit tests pass
- [ ] Tất cả integration tests pass
- [ ] Tất cả flow tests pass
- [ ] Test coverage > 95%
- [ ] Gas sử dụng nằm trong mức chấp nhận được
- [ ] Tất cả edge cases được test
- [ ] Cross-chain tests thành công trên testnet
- [ ] Security tests không phát hiện lỗ hổng
- [ ] Performance tests thỏa mãn yêu cầu

Hãy đảm bảo hoàn thành tất cả các test trước khi triển khai lên mainnet. 