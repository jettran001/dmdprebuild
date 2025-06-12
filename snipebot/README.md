# SnipeBot - DiamondChain

Bot giao dịch DeFi thông minh hỗ trợ đa blockchain

## API Endpoints

### Endpoint: /bugs
Method: GET  
Mô tả: Trả về danh sách các lỗi tiềm ẩn và trùng lặp trong codebase.

Ví dụ:
```bash
curl -X GET http://localhost:3000/bugs
```

Response:
```json
{
  "status": "success",
  "bugs": {
    "smart_trade/executor.rs": [
      "Chưa implement logic thực thi giao dịch tại nhiều vị trí (các dòng: 190, 405, 1071, 1721, 2627, 2842, 3485, 4234, 6208, 6957, 8652, 12310)"
    ],
    "mev_logic/types.rs": [
      "Import từ analys/mempool/types để tránh định nghĩa trùng lặp"
    ],
    "mev_logic/bot.rs": [
      "Hàm find_potential_failing_transactions: Nếu không có adapter hoặc mempool analyzer sẽ trả về lỗi hoặc Vec rỗng, cần log rõ hơn lý do và cảnh báo khi thiếu adapter/analyzer"
    ]
  },
  "timestamp": "2023-07-20T12:34:56.789Z"
}
```

### Endpoint: /analyze_token
Method: POST  
Mô tả: Phân tích an toàn của một token trên blockchain

Body:
```json
{
  "chain_id": 1,
  "token_address": "0x1234567890abcdef1234567890abcdef12345678"
}
```

Ví dụ:
```bash
curl -X POST http://localhost:3000/analyze_token \
  -H "Content-Type: application/json" \
  -d '{"chain_id": 1, "token_address": "0x1234567890abcdef1234567890abcdef12345678"}'
```

Response:
```json
{
  "status": "success",
  "is_safe": true,
  "summary": "Token an toàn: Không phát hiện vấn đề nghiêm trọng",
  "details": {
    "name": "Example Token",
    "symbol": "EX",
    "total_supply": "1000000000000000000000000",
    "verified": "true",
    "risk_score": "20.0",
    "recommendation": "Safe",
    "buy_tax": "0.00%",
    "sell_tax": "0.00%",
    "liquidity_usd": "$500000.00",
    "recent_buys": "45",
    "recent_sells": "38"
  },
  "timestamp": "2023-07-20T12:34:56.789Z"
}
```

## Cách sử dụng

### Cài đặt

1. Clone repository
```bash
git clone https://github.com/diamondchain/snipebot
cd snipebot
```

2. Tạo file cấu hình mặc định
```bash
cargo run -- init
```

3. Chỉnh sửa file cấu hình trong `config/bot_config.yaml`

### Chạy Bot

```bash
cargo run -- run
```

### Chỉ chạy API Server

```bash
cargo run -- api
```

### Test kết nối tới blockchain

```bash
cargo run -- test-chain --chain-id 1
``` 