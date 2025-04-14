//! 🧭 Entry Point: Đây là manifest chính chứa toàn bộ module của dự án network.
//! Mỗi thư mục là một đơn vị rõ ràng: `docker`, `nodes`, `protocols`, `messaging`, `edge`, `wasm`.
//! Bot hãy bắt đầu từ đây để resolve module path chính xác.
//! Được dùng làm tài liệu tham chiếu khi import từ domain khác (ví dụ: wallet, snipebot, blockchain).

/*
    network/
    ├── Cargo.toml                  -> Cấu hình dependencies
    ├── manifest.rs                 -> Tài liệu tham chiếu module path [liên quan: tất cả các module, BẮT BUỘC đọc đầu tiên]
    ├── lib.rs                      -> Khai báo module cấp cao, re-export [liên quan: tất cả module khác, điểm import cho crate]
    ├── src/docker/                 -> Quản lý các thiết lập cho Docker
    │   ├── mod.rs                  -> Khai báo submodule docker [liên quan: tất cả file trong docker]
    │   ├── compose.rs              -> Quản lý Docker Compose [liên quan: nodes]
    │   ├── containers.rs           -> Quản lý container lifecycle [liên quan: protocols, messaging]
    │   ├── network.rs              -> Cấu hình mạng cho Docker [liên quan: nodes, protocols]
    ├── src/nodes/                  -> Quản lý các node depin, master-slave
    │   ├── mod.rs                  -> Khai báo submodule nodes [liên quan: tất cả file trong nodes]
    │   ├── depin.rs                -> Quản lý node DePIN [liên quan: protocols, edge]
    │   ├── master.rs               -> Quản lý master node [liên quan: slave, protocols]
    │   ├── slave.rs                -> Quản lý slave node [liên quan: master, protocols]
    │   ├── discovery.rs            -> Phát hiện và kết nối node [liên quan: protocols]
    │   ├── health.rs               -> Giám sát sức khỏe node [liên quan: protocols, messaging]
    ├── src/protocols/              -> Quản lý các giao thức mạng
    │   ├── mod.rs                  -> Khai báo submodule protocols [liên quan: tất cả file trong protocols]
    │   ├── ipfs.rs                 -> Tương tác với IPFS [liên quan: edge, nodes]
    │   ├── quic.rs                 -> Cài đặt giao thức QUIC [liên quan: nodes, messaging]
    │   ├── grpc.rs                 -> Cài đặt giao thức gRPC [liên quan: nodes, messaging]
    │   ├── websocket.rs            -> Cài đặt WebSocket [liên quan: nodes, messaging, frontend]
    │   ├── redis.rs                -> Tương tác với Redis [liên quan: messaging, docker]
    │   ├── libp2p.rs               -> Tương tác với Libp2p [liên quan: nodes, edge]
    │   ├── webrtc.rs               -> Cài đặt WebRTC [liên quan: nodes, messaging]
    │   ├── coap.rs                 -> Cài đặt CoAP [liên quan: edge, nodes]
    ├── src/messaging/              -> Quản lý hệ thống messaging
    │   ├── mod.rs                  -> Khai báo submodule messaging [liên quan: tất cả file trong messaging]
    │   ├── mosquitto.rs            -> Tích hợp Mosquitto/Aedes [liên quan: protocols, nodes]
    │   ├── kafka.rs                -> Tích hợp Kafka [liên quan: protocols, docker]
    │   ├── zeromq.rs               -> Tích hợp ZeroMQ [liên quan: protocols, nodes]
    │   ├── messagepack.rs          -> Định dạng MessagePack [liên quan: protocols, wasm]
    │   ├── broker.rs               -> Message broker chung [liên quan: tất cả các messaging khác]
    ├── src/edge/                   -> Edge computing
    │   ├── mod.rs                  -> Khai báo submodule edge [liên quan: tất cả file trong edge]
    │   ├── compute.rs              -> Edge computing logic [liên quan: nodes, protocols]
    │   ├── sync.rs                 -> Đồng bộ dữ liệu edge [liên quan: protocols, messaging]
    │   ├── deployment.rs           -> Triển khai ứng dụng edge [liên quan: docker, wasm]
    ├── src/wasm/                   -> Web Assembly
    │   ├── mod.rs                  -> Khai báo submodule wasm [liên quan: tất cả file trong wasm]
    │   ├── runtime.rs              -> WASM runtime [liên quan: edge, nodes]
    │   ├── modules.rs              -> Quản lý WASM modules [liên quan: edge, protocols]
    │   ├── interop.rs              -> Tương tác giữa WASM và native [liên quan: messaging, protocols]

    Mối liên kết:
    - docker là cơ sở hạ tầng để triển khai các thành phần mạng
    - docker/compose.rs quản lý cấu hình Docker Compose cho triển khai mạng
    - docker/containers.rs quản lý vòng đời container cho protocols và messaging
    - docker/network.rs cấu hình mạng Docker cho nodes và protocols
    - nodes/mod.rs là cổng vào cho tất cả quản lý node
    - nodes/depin.rs quản lý các node DePIN tương tác với protocols và edge
    - nodes/master.rs và slave.rs quản lý cấu trúc master-slave
    - nodes/discovery.rs phát hiện và kết nối các node mới
    - nodes/health.rs giám sát trạng thái các node
    - protocols/mod.rs là cổng vào cho tất cả giao thức mạng
    - protocols/ipfs.rs tương tác với IPFS cho lưu trữ phân tán
    - protocols/quic.rs, grpc.rs, websocket.rs cài đặt các giao thức tương ứng
    - protocols/redis.rs tích hợp Redis cho caching
    - protocols/libp2p.rs tích hợp Libp2p cho mạng P2P
    - protocols/webrtc.rs cài đặt WebRTC cho kết nối trực tiếp
    - protocols/coap.rs cài đặt CoAP cho IoT và edge
    - messaging/mod.rs là cổng vào cho tất cả hệ thống nhắn tin
    - messaging/mosquitto.rs tích hợp Mosquitto/Aedes MQTT
    - messaging/kafka.rs tích hợp Kafka cho xử lý tin nhắn quy mô lớn
    - messaging/zeromq.rs tích hợp ZeroMQ cho messaging hiệu suất cao
    - messaging/messagepack.rs định dạng MessagePack hiệu quả
    - messaging/broker.rs tầng trừu tượng chung cho các hệ thống nhắn tin
    - edge/mod.rs là cổng vào cho tất cả edge computing
    - edge/compute.rs logic xử lý edge computing
    - edge/sync.rs đồng bộ dữ liệu giữa edge và core
    - edge/deployment.rs triển khai ứng dụng tới edge
    - wasm/mod.rs là cổng vào cho tất cả WebAssembly
    - wasm/runtime.rs quản lý môi trường thực thi WASM
    - wasm/modules.rs quản lý các module WASM
    - wasm/interop.rs tương tác giữa WASM và native code
    - network tương tác với wallet để xác thực và ký giao dịch
    - network cung cấp API cho snipebot để thực hiện giao dịch mạng
    - network tương tác với blockchain để theo dõi và xác minh trạng thái
*/

// Module structure của dự án network
pub mod docker;    // Quản lý các thiết lập cho Docker
pub mod nodes;     // Quản lý các node depin, master-slave
pub mod protocols; // Quản lý các giao thức mạng
pub mod messaging; // Quản lý hệ thống messaging
pub mod edge;      // Edge computing
pub mod wasm;      // Web Assembly

/**
 * Hướng dẫn import:
 * 
 * 1. Import từ internal crates:
 * - use crate::docker::compose::DockerCompose;
 * - use crate::docker::containers::ContainerManager;
 * - use crate::nodes::depin::DePinNode;
 * - use crate::nodes::master::MasterNode;
 * - use crate::nodes::slave::SlaveNode;
 * - use crate::protocols::ipfs::IpfsClient;
 * - use crate::protocols::grpc::GrpcServer;
 * - use crate::messaging::kafka::KafkaProducer;
 * - use crate::edge::compute::EdgeProcessor;
 * - use crate::wasm::runtime::WasmRuntime;
 * 
 * 2. Import từ external crates:
 * - use wallet::walletmanager::api::WalletManagerApi;
 * - use blockchain::smartcontracts::diamond_nft::DiamondNFT;
 * - use snipebot::chain_adapters::evm_adapter::EvmAdapter;
 * 
 * 3. Import từ third-party libraries:
 * - use tokio::net::{TcpListener, TcpStream};
 * - use ipfs_api_backend_hyper::{IpfsApi, IpfsClient};
 * - use tonic::{transport::Server, Request, Response, Status};
 * - use rdkafka::producer::{FutureProducer, FutureRecord};
 * - use wasmer::{Store, Module, Instance};
 * - use libp2p::{Swarm, identity, PeerId};
 */

// Network Module - DiamondChain
// Cung cấp cơ sở hạ tầng mạng, giao thức và dịch vụ kết nối cho hệ sinh thái DiamondChain

/// Module Network - Quản lý và triển khai tất cả các thành phần liên quan đến kết nối mạng
/// cho hệ sinh thái DiamondChain, bao gồm các giao thức, dịch vụ mạng phân tán và
/// các giải pháp truyền thông nhanh, an toàn và đáng tin cậy.
pub struct NetworkManifest {
    /// Quản lý triển khai các container Docker cho các thành phần mạng
    pub docker: DockerModule,
    
    /// Quản lý các node mạng DePIN và cấu trúc master-slave
    pub nodes: NodesModule,
    
    /// Các giao thức mạng được hỗ trợ trong hệ sinh thái
    pub protocols: ProtocolsModule,
    
    /// Các giải pháp truyền tin và giao tiếp giữa các thành phần
    pub messaging: MessagingModule,
    
    /// Edge computing và các giải pháp tính toán phân tán
    pub edge: EdgeModule,
    
    /// Hỗ trợ WebAssembly cho các thành phần mạng
    pub wasm: WasmModule,
}

/// Quản lý các container Docker cho việc triển khai các dịch vụ mạng
pub struct DockerModule {
    /// Docker compose cho triển khai đa container
    pub compose: DockerComposeService,
    
    /// Quản lý container cho các dịch vụ mạng
    pub containers: NetworkContainerService,
    
    /// Cấu hình mạng Docker cho các dịch vụ
    pub networks: DockerNetworkService,
}

/// Quản lý các node mạng trong hệ sinh thái DiamondChain
pub struct NodesModule {
    /// Quản lý node DePIN (Decentralized Physical Infrastructure Network)
    pub depin: DePINNodeService,
    
    /// Quản lý cấu trúc Master-Slave cho các node
    pub master_slave: MasterSlaveService,
    
    /// Quản lý khả năng mở rộng và cân bằng tải
    pub scaling: NodeScalingService,
    
    /// Dịch vụ khám phá và đăng ký node
    pub discovery: NodeDiscoveryService,
}

/// Các giao thức mạng được hỗ trợ trong hệ sinh thái
pub struct ProtocolsModule {
    /// IPFS (InterPlanetary File System) cho lưu trữ phân tán
    pub ipfs: IPFSService,
    
    /// QUIC protocol cho truyền tải dữ liệu nhanh
    pub quic: QUICService,
    
    /// gRPC cho giao tiếp dịch vụ
    pub grpc: GRPCService,
    
    /// WebSocket cho kết nối hai chiều thời gian thực
    pub websocket: WebSocketService,
    
    /// Redis cho cache và pub/sub
    pub redis: RedisService,
    
    /// Libp2p cho giao tiếp peer-to-peer
    pub libp2p: Libp2pService,
    
    /// WebRTC cho giao tiếp trực tiếp giữa trình duyệt
    pub webrtc: WebRTCService,
    
    /// CoAP (Constrained Application Protocol) cho IoT
    pub coap: CoAPService,
}

/// Các giải pháp truyền tin và giao tiếp
pub struct MessagingModule {
    /// Mosquitto/Aedes MQTT broker
    pub mqtt: MQTTService,
    
    /// Apache Kafka cho xử lý dữ liệu luồng
    pub kafka: KafkaService,
    
    /// ZeroMQ cho truyền tin độ trễ thấp
    pub zeromq: ZeroMQService,
    
    /// MessagePack cho định dạng dữ liệu hiệu quả
    pub messagepack: MessagePackService,
}

/// Edge computing và các giải pháp tính toán phân tán
pub struct EdgeModule {
    /// Quản lý tính toán tại các thiết bị Edge
    pub computing: EdgeComputingService,
    
    /// Đồng bộ hóa dữ liệu giữa edge và cloud
    pub sync: EdgeSyncService,
    
    /// Tối ưu hóa băng thông và độ trễ
    pub optimization: EdgeOptimizationService,
}

/// Hỗ trợ WebAssembly cho các thành phần mạng
pub struct WasmModule {
    /// Runtime WASM cho mạng
    pub runtime: WasmRuntimeService,
    
    /// Biên dịch và tối ưu hóa WASM
    pub compiler: WasmCompilerService,
    
    /// Tích hợp WASM với các dịch vụ mạng
    pub integration: WasmIntegrationService,
}

// Chi tiết về các dịch vụ được triển khai trong các file tương ứng
// trong thư mục src của module network
