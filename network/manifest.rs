// =========================
// NETWORK DOMAIN MANIFEST (Cập nhật: 2024-09-18)
// =========================

//! 🧭 Domain Network: Quản lý giao tiếp, bảo mật, cấu hình, điều phối và quản lý node trong hệ thống phân tán.
//! 
//! # Cấu trúc Domain (thực tế)
//! 
//! network/
//! ├── core/                     # Core engine, trait, logic chính
//! │   ├── engine.rs            # NetworkEngine trait, DefaultNetworkEngine, PluginType, PluginId, PluginStatus, NetworkMessage, MessageType
//! │   └── utils.rs             # Utilities cho core
//! ├── config/                   # Cấu hình tập trung domain network
//! │   ├── mod.rs              # NetworkConfig tổng hợp, validate, load từ file/env
//! │   ├── core.rs             # NetworkCoreConfig (cấu hình lõi cho engine)
//! │   ├── types.rs            # Các types cấu hình plugin/service
//! │   ├── loader.rs           # ConfigLoader: tải, validate, hợp nhất config
//! │   └── error.rs            # ConfigError tập trung
//! ├── infra/                    # Hạ tầng, trait chuẩn, mock, pool, profile
//! │   ├── service_traits.rs    # Định nghĩa trait chuẩn hóa cho service (Messaging, Node, Plugin, v.v.)
//! │   ├── service_mocks.rs     # Mock service cho test/dev
//! │   ├── redis_pool.rs        # Redis pool abstraction
//! │   └── toml_profile.rs      # Đọc profile từ file toml
//! ├── plugins/                  # Các plugin mạng (implement trait Plugin)
//! │   ├── pool.rs              # Connection pooling plugin
//! │   ├── grpc.rs              # gRPC plugin
//! │   ├── wasm.rs              # WebAssembly plugin
//! │   ├── libp2p.rs            # P2P plugin
//! │   ├── webrtc.rs            # WebRTC plugin
//! │   ├── ws.rs                # WebSocket plugin
//! │   └── redis.rs             # Redis plugin
//! ├── security/                 # Bảo mật: xác thực, validation, rate limit
//! │   ├── input_validation.rs  # Trait Validator, Sanitizer, các hàm check_xss, check_sql_injection, validate input
//! │   ├── auth_middleware.rs   # AuthService, AuthError, UserRole, JWT, API key, Simple token, middleware cho warp
//! │   ├── api_validation.rs    # ApiValidator, FieldRule, các rule validation cho API endpoint
//! │   ├── rate_limiter.rs      # RateLimiter, RateLimitConfig, các thuật toán giới hạn tốc độ
//! │   ├── jwt.rs               # JwtService, JwtConfig
//! │   └── token.rs             # TokenService, TokenConfig
//! ├── node_manager/            # Quản lý node (Master/Slave/Discovery/Scheduler)
//! │   ├── master.rs            # PartitionedMasterNodeService, trait, logic master node
//! │   ├── slave.rs             # SlaveNodeService, trait, logic slave node
//! │   ├── discovery.rs         # DefaultDiscoveryService, node discovery
//! │   ├── scheduler.rs         # DefaultSchedulerService, task scheduling
//! │   ├── node_classifier.rs   # Phân loại node
//! │   └── execution_adapter.rs # Adapter cho thực thi task
//! ├── messaging/               # Truyền tin (MQTT, Kafka)
//! │   ├── mqtt.rs             # MessagingMqttService, impl trait, logic MQTT
//! │   └── kafka.rs            # MessagingKafkaService, impl trait, logic Kafka
//! ├── dispatcher/              # Điều phối task, event
//! │   ├── dispatcher.rs       # Task dispatcher, trait, logic điều phối
//! │   └── event_router.rs     # Event router, định tuyến sự kiện
//! ├── logs/                    # Logging, lưu trữ log
//! │   └── mod.rs              # Logging utilities
//! ├── errors.rs                # NetworkError, error tập trung toàn domain
//! ├── api.rs                   # Định nghĩa các route API, mapping tới service/engine
//! ├── main.rs                  # Entry point khởi động network engine
//! └── lib.rs                   # Export các module chính
//!
//! # Mô tả chức năng từng module
//!
//! - **core/**: Định nghĩa trait NetworkEngine, Plugin, các type cốt lõi, quản lý plugin, message, trạng thái engine.
//! - **config/**: Cấu hình tập trung, validate, load từ nhiều nguồn, hỗ trợ migration, error tập trung cho config.
//! - **infra/**: Chuẩn hóa trait service, mock cho test, pool cho Redis, profile cho node/service.
//! - **plugins/**: Các plugin mạng (WebSocket, gRPC, WebRTC, Libp2p, Redis, Wasm), mỗi plugin implement trait Plugin, có health check, metrics, shutdown.
//! - **security/**: Bảo mật toàn diện: xác thực (JWT, API key, Simple token), validation input, rate limiting, các hàm kiểm tra XSS/SQLi, middleware cho API.
//! - **node_manager/**: Quản lý node master/slave, discovery, scheduler, phân loại node, adapter thực thi task, đồng bộ trạng thái node.
//! - **messaging/**: Dịch vụ truyền tin MQTT/Kafka, chuẩn hóa trait, đồng bộ interface với node_manager, callback API.
//! - **dispatcher/**: Điều phối task, event, định tuyến sự kiện, quản lý luồng công việc phân tán.
//! - **logs/**: Tiện ích logging, lưu trữ log tập trung, hỗ trợ tracing.
//! - **errors.rs**: Định nghĩa NetworkError, các error type tập trung, chuẩn hóa error handling toàn domain.
//! - **api.rs**: Định nghĩa các route API, ánh xạ tới các service/engine, validate input, trả về error chuẩn hóa.
//! - **main.rs**: Entry point khởi động engine, load config, khởi tạo service, start network.
//!
//! # Các trait & mối liên hệ chính
//!
//! - `NetworkEngine`: trait chính cho engine, quản lý plugin, message, trạng thái, metrics.
//! - `Plugin`: trait cho các plugin mạng, phải implement các hàm khởi tạo, xử lý message, health check.
//! - `MessagingMqttService`, `MessagingKafkaService`, `MasterNodeService`, `SlaveNodeService`: trait chuẩn hóa cho messaging/node_manager, chỉ định nghĩa tại infra/service_traits.rs.
//! - `Validator`, `Sanitizer`: trait cho validation input, bảo mật.
//! - `AuthService`: service xác thực, middleware cho API, tích hợp JWT, API key, Simple token.
//!
//! # Các route/API chính (api.rs)
//! - `/api/plugins`         : Quản lý plugin (list, register, unregister, health check)
//! - `/api/nodes`           : Quản lý node (list, info, register, unregister)
//! - `/api/metrics`         : Lấy metrics hệ thống, plugin, node
//! - `/api/logs`            : Truy xuất log
//! - `/api/auth/*`          : Xác thực, cấp phát token, validate token
//! - `/api/dispatcher/*`    : Điều phối task, event
//! - `/api/message/*`       : Gửi/nhận message qua MQTT/Kafka
//!
//! # Quy tắc Domain & Best Practice
//! - Mọi service phải implement trait chuẩn hóa, không lặp lại trait ở nhiều nơi
//! - Không tạo file/module ngoài manifest khi chưa được duyệt
//! - Tất cả error phải dùng enum, không dùng String tự do
//! - Validate input, auth, rate limit cho mọi API
//! - Logging, tracing đầy đủ cho mọi event quan trọng
//! - Cấu hình tập trung, không hardcode giá trị nhạy cảm
//! - Test coverage cho từng module, integration test cho plugin/service
