// Config module (cập nhật 2024-09-13)
//! # Config Module
//! 
//! **QUAN TRỌNG: Module này đã được di chuyển từ `network/infra/config` lên `network/config`.**
//! 
//! Module cấu hình tập trung cho toàn bộ domain network. Được nâng lên cấp cao hơn 
//! (ra khỏi `infra`) để dễ dàng import và sử dụng bởi tất cả các module khác 
//! (core, plugins, security, node_manager, messaging, dispatcher).
//! 
//! ## Cách cập nhật code cũ
//! 
//! - Thay thế tất cả `use crate::infra::config::*` thành `use crate::config::*`
//! - Thay thế tất cả `use crate::infra::config::types::*` thành `use crate::config::types::*`
//! - Thay thế tất cả `use crate::infra::config::core::*` thành `use crate::config::core::*`
//! - Thay thế tất cả `use crate::infra::config::error::*` thành `use crate::config::error::*`
//! - Thay thế tất cả `use crate::infra::config::loader::*` thành `use crate::config::loader::*`
//!
//! ## Lịch sử hợp nhất
//!
//! Module này là kết quả của việc hợp nhất các file sau:
//! - network/core/config.rs
//! - network/infra/config.rs
//! - network/infra/config_types.rs
//! - network/infra/config_loader.rs
//! - network/security/configs/*.yaml
//! 
//! Module này được thiết kế làm điểm duy nhất cho tất cả cấu hình liên quan đến network domain.
//!
//! Module này cung cấp cấu trúc cấu hình tập trung sau khi hợp nhất các module config khác.
//! Module này thay thế các module config trùng lặp trước đây:
//! - network/core/config.rs
//! - network/infra/config.rs
//! - network/infra/config_types.rs
//! - network/infra/config_loader.rs
//!
//! Cấu trúc thống nhất:
//! - types.rs: Các type config chi tiết cho các plugin
//! - core.rs: Định nghĩa NetworkCoreConfig, hợp nhất từ network/core/config.rs
//! - loader.rs: Hệ thống ConfigLoader thống nhất
//! - error.rs: Định nghĩa ConfigError tập trung
//! - mod.rs: (file này) Export tất cả và định nghĩa NetworkConfig tổng hợp
//!
//! # Hướng dẫn di chuyển từ cấu trúc cũ
//!
//! ## 1. Di chuyển từ network/core/config.rs
//! ```rust
//! // Cách cũ
//! use crate::core::config::{NetworkConfig, ConfigLoader};
//! 
//! // Cách mới
//! use crate::infra::config::core::NetworkCoreConfig;
//! use crate::infra::config::loader::ConfigLoader;
//! use crate::infra::config::NetworkConfig;
//! ```
//!
//! ## 2. Di chuyển từ file YAML trong network/security/configs
//! ```rust
//! // Cách cũ: Tải trực tiếp từ các file YAML riêng lẻ
//! // let content = std::fs::read_to_string("network/security/configs/auth.yaml")?;
//! // let auth_config: AuthConfig = serde_yaml::from_str(&content)?;
//! 
//! // Cách mới: Sử dụng ConfigLoader thống nhất
//! use crate::infra::config::loader::ConfigLoader;
//! 
//! let config_loader = ConfigLoader::new();
//! let config = config_loader.load_all("config/network.json")?;
//! // Truy cập thông tin bảo mật
//! if let Some(auth) = &config.security.auth {
//!     // Xử lý auth
//! }
//! ```
//!
//! ## 3. Ví dụ chi tiết cách chuyển đổi từng loại cấu hình
//!
//! ### Chuyển đổi trong DefaultNetworkEngine
//! ```rust
//! // Cách cũ:
//! use crate::core::types::NetworkConfig;
//! 
//! pub struct DefaultNetworkEngine {
//!     config: Arc<RwLock<NetworkConfig>>,
//!     // Các trường khác...
//! }
//! 
//! impl DefaultNetworkEngine {
//!     pub fn new() -> Self {
//!         Self {
//!             config: Arc::new(RwLock::new(NetworkConfig {
//!                 node_id: String::new(),
//!                 // Các trường khác...
//!             })),
//!             // Khởi tạo các trường khác...
//!         }
//!     }
//! }
//! 
//! // Cách mới:
//! use crate::infra::config::{NetworkConfig as InfraNetworkConfig, core::NetworkCoreConfig};
//! 
//! pub struct DefaultNetworkEngine {
//!     config: Arc<RwLock<InfraNetworkConfig>>,
//!     // Các trường khác...
//! }
//! 
//! impl DefaultNetworkEngine {
//!     pub fn new() -> Self {
//!         Self {
//!             config: Arc::new(RwLock::new(InfraNetworkConfig::default())),
//!             // Khởi tạo các trường khác...
//!         }
//!     }
//! }
//! ```
//!
//! ### Chuyển đổi trong NetworkEngine trait và impl
//! ```rust
//! // Cách cũ:
//! trait NetworkEngine {
//!     async fn init(&self, config: &NetworkConfig) -> Result<()>;
//!     // Các phương thức khác...
//! }
//! 
//! // Cách mới:
//! trait NetworkEngine {
//!     async fn init(&self, config: &NetworkCoreConfig) -> Result<()>;
//!     // Các phương thức khác...
//! }
//! 
//! // Cách cũ trong implement:
//! #[async_trait]
//! impl NetworkEngine for DefaultNetworkEngine {
//!     async fn init(&self, config: &NetworkConfig) -> Result<()> {
//!         let mut config_guard = self.config.write().await;
//!         *config_guard = config.clone();
//!         Ok(())
//!     }
//! }
//! 
//! // Cách mới trong implement:
//! #[async_trait]
//! impl NetworkEngine for DefaultNetworkEngine {
//!     async fn init(&self, config: &NetworkCoreConfig) -> Result<()> {
//!         let mut config_guard = self.config.write().await;
//!         
//!         // Chuyển đổi từ NetworkCoreConfig sang InfraNetworkConfig
//!         let mut new_config = InfraNetworkConfig::default();
//!         new_config.core = config.clone();
//!         
//!         *config_guard = new_config;
//!         Ok(())
//!     }
//! }
//! ```
//!
//! ### Chuyển đổi khi tải cấu hình bảo mật
//! ```rust
//! // Cách cũ (tải từng file riêng lẻ):
//! let auth_content = std::fs::read_to_string("network/security/configs/auth.yaml")?;
//! let auth_yaml: serde_yaml::Value = serde_yaml::from_str(&auth_content)?;
//! let auth_config = auth_yaml["auth"].clone();
//! 
//! let rate_limit_content = std::fs::read_to_string("network/security/configs/rate_limiter.yaml")?;
//! let rate_limit_yaml: serde_yaml::Value = serde_yaml::from_str(&rate_limit_content)?;
//! let rate_limit_config = rate_limit_yaml["rate_limiter"].clone();
//! 
//! // Cách mới (tải thống nhất từ ConfigLoader):
//! let config_loader = ConfigLoader::new();
//! let config = config_loader.load_all("config/network.json")?;
//! 
//! // Truy cập AuthConfig
//! if let Some(auth) = &config.security.auth {
//!     let jwt_enabled = auth.methods.jwt.enabled;
//!     // Sử dụng auth...
//! }
//! 
//! // Truy cập RateLimitConfig
//! let rate_limit = &config.security.rate_limit;
//! if rate_limit.enabled {
//!     let max_requests = rate_limit.max_requests;
//!     // Sử dụng rate limit...
//! }
//! ```
//!
//! ## 4. Quy tắc sử dụng
//! 
//! - Luôn sử dụng ConfigLoader để tải cấu hình.
//! - Ưu tiên sử dụng load_all() để tải từ tất cả các nguồn.
//! - ConfigLoader sẽ tự động tải cấu hình từ các file YAML trong network/security/configs.
//! - Với các tính năng hiện chỉ có trong NetworkCoreConfig, truy cập qua config.core.
//!
//! # Lưu ý về khả năng tương thích
//!
//! - Tất cả các module cũ đã được đánh dấu @deprecated.
//! - Các module cũ sẽ được giữ lại tạm thời để đảm bảo tương thích ngược.
//! - Dự kiến các module cũ sẽ bị xóa sau ngày 2024-12-31.
//! - Nếu có vấn đề khi chuyển đổi, vui lòng liên hệ team phát triển.
//!
//! # Thời hạn loại bỏ hoàn toàn code cũ
//!
//! - NetworkConfig trong network/core/types.rs: Xóa sau 2024-12-31
//! - NetworkConfig trong network/core/engine.rs: Xóa sau 2024-12-31
//! - Tất cả tham chiếu đến các struct trên phải được chuyển đổi trước thời hạn này

pub mod error;
pub mod types;
pub mod core;
pub mod loader;

use std::collections::HashMap;
use std::path::Path;
use serde::{Serialize, Deserialize};
use tracing::debug;

// Re-export từ các module con
pub use self::error::ConfigError;
pub use self::core::NetworkCoreConfig;
pub use self::loader::ConfigLoader;

/// NetworkConfig tổng hợp - Định nghĩa cấu hình toàn diện cho network domain
/// 
/// Struct này hợp nhất các cấu hình từ core và infra để tạo một cấu hình đầy đủ cho toàn bộ hệ thống
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetworkConfig {
    /// Cấu hình core engine
    #[serde(default)]
    pub core: NetworkCoreConfig,
    
    /// Cấu hình các plugin
    #[serde(default)]
    pub plugins: PluginsConfig,
    
    /// Cấu hình bảo mật
    #[serde(default)]
    pub security: SecurityConfig,
    
    /// Cấu hình giám sát
    #[serde(default)]
    pub monitoring: MonitoringConfig,
    
    /// Cấu hình hạ tầng
    #[serde(default)]
    pub infra: InfraConfig,
    
    /// Cấu hình logging
    #[serde(default)]
    pub logging: LoggingConfig,
}

/// Cấu hình tổng hợp cho các plugins
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginsConfig {
    /// Cấu hình Redis
    #[serde(default)]
    pub redis: RedisConfig,
    
    /// Cấu hình WebRTC
    #[serde(default)]
    pub webrtc: WebrtcConfig,
    
    /// Cấu hình IPFS
    #[serde(default)]
    pub ipfs: IpfsConfig,
    
    /// Cấu hình WASM
    #[serde(default)]
    pub wasm: WasmConfig,
    
    /// Cấu hình Libp2p
    #[serde(default)]
    pub libp2p: Libp2pConfig,
    
    /// Cấu hình gRPC
    #[serde(default)]
    pub grpc: GrpcConfig,
}

/// Cấu hình giám sát
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MonitoringConfig {
    /// Bật/tắt metrics
    #[serde(default = "default_true")]
    pub enable_metrics: bool,
    
    /// Cổng metrics
    #[serde(default = "default_metrics_port")]
    pub metrics_port: u16,
    
    /// Đường dẫn metrics
    #[serde(default = "default_metrics_path")]
    pub metrics_path: String,
    
    /// Bật/tắt health check
    #[serde(default = "default_true")]
    pub enable_health_checks: bool,
    
    /// Cổng health check
    #[serde(default = "default_health_port")]
    pub health_port: u16,
    
    /// Đường dẫn health check
    #[serde(default = "default_health_path")]
    pub health_path: String,
}

/// Cấu hình bảo mật
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SecurityConfig {
    /// Đường dẫn chứng chỉ TLS
    #[serde(default)]
    pub tls_cert_path: Option<String>,
    
    /// Đường dẫn khóa TLS
    #[serde(default)]
    pub tls_key_path: Option<String>,
    
    /// Bật/tắt xác thực
    #[serde(default)]
    pub enable_auth: bool,
    
    /// Endpoint xác thực token
    #[serde(default)]
    pub auth_endpoint: Option<String>,
    
    /// Cấu hình rate limit
    #[serde(default)]
    pub rate_limit: RateLimitConfig,
    
    /// Cấu hình CORS
    #[serde(default)]
    pub cors: CorsConfig,

    /// Cấu hình xác thực chi tiết từ auth.yaml
    #[serde(default)]
    pub auth: Option<AuthConfig>,
    
    /// Cấu hình validation đầu vào từ input_validation.yaml
    #[serde(default)]
    pub input_validation: Option<InputValidationConfig>,
}

/// Cấu hình xác thực chi tiết từ auth.yaml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Phương pháp xác thực
    pub methods: AuthMethods,
    
    /// Cấu hình role và quyền
    #[serde(default)]
    pub roles: HashMap<String, RoleConfig>,
    
    /// Endpoint bảo mật
    #[serde(default)]
    pub protected_endpoints: Vec<ProtectedEndpoint>,
    
    /// Xử lý token
    pub token_management: TokenManagement,
}

/// Phương pháp xác thực
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthMethods {
    /// JWT authentication
    pub jwt: JwtConfig,
    
    /// API Key authentication
    pub api_key: ApiKeyConfig,
    
    /// Simple token authentication
    pub simple_token: SimpleTokenConfig,
}

/// Cấu hình JWT
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtConfig {
    /// Bật/tắt JWT
    pub enabled: bool,
    
    /// Khóa bí mật
    pub secret: String,
    
    /// Thuật toán mã hóa
    pub algorithm: String,
    
    /// Issuer
    pub issuer: String,
    
    /// Audience
    pub audience: String,
    
    /// Thời gian hết hạn (phút)
    pub token_expiry_minutes: u64,
    
    /// Thời gian hết hạn của refresh token (ngày)
    pub refresh_token_expiry_days: u64,
}

/// Cấu hình API Key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyConfig {
    /// Bật/tắt API Key
    pub enabled: bool,
    
    /// Header chứa API Key
    pub header_name: String,
    
    /// Prefix của API Key
    pub prefix: String,
    
    /// Độ dài khóa
    pub key_length: u32,
}

/// Cấu hình simple token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleTokenConfig {
    /// Bật/tắt simple token
    pub enabled: bool,
    
    /// Header chứa token
    pub header_name: String,
    
    /// Kiểm tra IP
    pub check_ip: bool,
    
    /// Thời gian hết hạn (phút)
    pub token_expiry_minutes: u64,
}

/// Cấu hình role
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleConfig {
    /// Mô tả
    pub description: String,
    
    /// Danh sách quyền
    pub permissions: Vec<String>,
}

/// Cấu hình endpoint bảo mật
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtectedEndpoint {
    /// Đường dẫn
    pub path: String,
    
    /// Các phương thức HTTP
    pub methods: Vec<String>,
    
    /// Các role được phép truy cập
    pub roles: Vec<String>,
}

/// Cấu hình quản lý token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenManagement {
    /// Khoảng thời gian xóa token hết hạn (phút)
    pub clean_expired_interval_minutes: u64,
    
    /// Số lượng token tối đa cho mỗi người dùng
    pub max_tokens_per_user: u32,
    
    /// Cho phép đăng nhập nhiều lần
    pub allow_multiple_logins: bool,
}

/// Rate limit config
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RateLimitConfig {
    /// Bật/tắt rate limit
    #[serde(default)]
    pub enabled: bool,
    
    /// Số request tối đa trong một khoảng thời gian
    #[serde(default = "default_max_requests")]
    pub max_requests: usize,
    
    /// Khoảng thời gian tính bằng giây
    #[serde(default = "default_window_seconds")]
    pub window_seconds: u64,
    
    /// Thuật toán mặc định
    #[serde(default)]
    pub default_algorithm: Option<String>,
    
    /// Hành động mặc định khi vượt quá giới hạn
    #[serde(default)]
    pub default_action: Option<String>,
    
    /// HTTP status code khi reject
    #[serde(default)]
    pub reject_status_code: Option<u16>,
    
    /// Thông báo khi reject
    #[serde(default)]
    pub reject_message: Option<String>,
    
    /// Bao gồm các header trong response
    #[serde(default)]
    pub include_headers: Option<bool>,
    
    /// Header số request còn lại
    #[serde(default)]
    pub remaining_header: Option<String>,
    
    /// Header giới hạn request
    #[serde(default)]
    pub limit_header: Option<String>,
    
    /// Header thời gian reset
    #[serde(default)]
    pub reset_header: Option<String>,
    
    /// Cấu hình chi tiết cho từng đường dẫn API
    #[serde(default)]
    pub paths: Option<Vec<RateLimitPath>>,
    
    /// Cách xác định client
    #[serde(default)]
    pub identifier: Option<RateLimitIdentifier>,
    
    /// Cấu hình lưu trữ trạng thái
    #[serde(default)]
    pub storage: Option<RateLimitStorage>
}

/// Cấu hình rate limit cho một đường dẫn API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitPath {
    /// Đường dẫn
    pub path: String,
    
    /// Thuật toán
    pub algorithm: String,
    
    /// Giới hạn request
    pub limit: u32,
    
    /// Cửa sổ thời gian (giây)
    pub window: u64,
    
    /// Hành động khi vượt quá giới hạn
    pub action: String,
}

/// Cách xác định client cho rate limit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitIdentifier {
    /// Sử dụng IP
    pub use_ip: bool,
    
    /// Sử dụng API key
    pub use_api_key: bool,
    
    /// Sử dụng ID người dùng
    pub use_user_id: bool,
    
    /// Danh sách IP proxy tin cậy
    pub trusted_proxy_ips: Vec<String>,
    
    /// Header chứa IP thực
    pub header_for_real_ip: String,
}

/// Cấu hình lưu trữ trạng thái rate limit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitStorage {
    /// Loại lưu trữ (memory, redis)
    pub type_: String,
    
    /// Khoảng thời gian xóa trạng thái cũ (giây)
    pub cleanup_interval: u64,
    
    /// URL Redis
    pub redis_url: Option<String>,
    
    /// Prefix cho key Redis
    pub redis_prefix: Option<String>,
}

/// Cấu hình CORS
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CorsConfig {
    /// Bật/tắt CORS
    #[serde(default = "default_true")]
    pub enabled: bool,
    
    /// Các nguồn được phép (để trống = tất cả)
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    
    /// Các phương thức được phép
    #[serde(default = "default_http_methods")]
    pub allowed_methods: Vec<String>,
    
    /// Các header được phép
    #[serde(default = "default_http_headers")]
    pub allowed_headers: Vec<String>,
    
    /// Cho phép credentials
    #[serde(default)]
    pub allow_credentials: bool,
}

/// Cấu hình hạ tầng
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InfraConfig {
    /// Cấu hình nginx
    pub nginx: Option<NginxConfig>,
    
    /// Cấu hình Prometheus
    pub prometheus: Option<PrometheusConfig>,
    
    /// Cấu hình Grafana
    pub grafana: Option<GrafanaConfig>,
}

/// Cấu hình nginx
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NginxConfig {
    /// Cổng mở
    #[serde(default = "default_nginx_port")]
    pub port: u16,
    
    /// Bật SSL
    #[serde(default)]
    pub ssl_enabled: bool,
}

fn default_nginx_port() -> u16 { 80 }

/// Cấu hình Prometheus
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PrometheusConfig {
    /// Khoảng thời gian scrape (giây)
    #[serde(default = "default_prometheus_scrape")]
    pub scrape_interval: u64,
    
    /// Cổng
    #[serde(default = "default_prometheus_port")]
    pub port: u16,
    
    /// Thời gian lưu giữ (ngày)
    #[serde(default = "default_prometheus_retention")]
    pub retention_days: u64,
}

fn default_prometheus_scrape() -> u64 { 15 }
fn default_prometheus_port() -> u16 { 9090 }
fn default_prometheus_retention() -> u64 { 15 }

/// Cấu hình Grafana
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GrafanaConfig {
    /// Cổng
    #[serde(default = "default_grafana_port")]
    pub port: u16,
    
    /// Mật khẩu admin
    #[serde(default = "default_grafana_password")]
    pub admin_password: String,
    
    /// Cho phép truy cập không xác thực
    #[serde(default)]
    pub anonymous_access: bool,
}

fn default_grafana_port() -> u16 { 3000 }
fn default_grafana_password() -> String { "admin".to_string() }

/// Cấu hình logging
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LoggingConfig {
    /// Cấp độ log
    pub level: String,
    
    /// Định dạng log
    pub format: String,
    
    /// File log
    pub log_file: Option<String>,
}

/// Cấu hình validation đầu vào từ input_validation.yaml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputValidationConfig {
    /// Bật/tắt validation
    pub enabled: bool,
    
    /// Chế độ strict (mọi lỗi đều dẫn đến từ chối request)
    pub strict_mode: bool,
    
    /// Xử lý khi có lỗi validation (reject, sanitize, log_only)
    pub on_error: String,
    
    /// HTTP status code khi reject
    pub error_status_code: u16,
    
    /// Trả về chi tiết lỗi trong response
    pub return_errors: bool,
    
    /// Giới hạn request
    pub request_limits: RequestLimits,
    
    /// Các trường thường gặp
    pub common_fields: HashMap<String, ValidationFieldConfig>,
    
    /// JSON schema cho các API endpoint
    pub schemas: HashMap<String, serde_json::Value>,
    
    /// Cấu hình bảo mật
    pub security: ValidationSecurityConfig,
}

/// Giới hạn request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestLimits {
    /// Kích thước tối đa của body
    pub max_body_size: usize,
    
    /// Số lượng header tối đa
    pub max_headers: usize,
    
    /// Kích thước tối đa của header
    pub max_header_size: usize,
    
    /// Số lượng query parameter tối đa
    pub max_query_params: usize,
    
    /// Số lượng field form tối đa
    pub max_form_fields: usize,
    
    /// Độ sâu tối đa của JSON
    pub max_json_depth: usize,
}

/// Cấu hình field validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationFieldConfig {
    /// Pattern regex
    pub pattern: Option<String>,
    
    /// Độ dài tối đa
    pub max_length: Option<usize>,
    
    /// Độ dài tối thiểu
    pub min_length: Option<usize>,
    
    /// Bắt buộc
    pub required: Option<bool>,
    
    /// Sanitize
    pub sanitize: Option<bool>,
    
    /// Yêu cầu chữ hoa
    pub require_uppercase: Option<bool>,
    
    /// Yêu cầu chữ thường
    pub require_lowercase: Option<bool>,
    
    /// Yêu cầu số
    pub require_number: Option<bool>,
    
    /// Yêu cầu ký tự đặc biệt
    pub require_special: Option<bool>,
}

/// Cấu hình bảo mật validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationSecurityConfig {
    /// Phát hiện XSS
    pub xss_detection: SecurityDetectionConfig,
    
    /// Phát hiện SQL Injection
    pub sql_injection: SecurityDetectionConfig,
    
    /// Phát hiện dữ liệu nhạy cảm
    pub sensitive_data: SecurityDetectionConfig,
}

/// Security detection config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityDetectionConfig {
    /// Bật/tắt
    pub enabled: bool,
    
    /// Sanitize
    pub sanitize: Option<bool>,
    
    /// Alert only
    pub alert_only: Option<bool>,
    
    /// Danh sách pattern
    pub patterns: Vec<String>,
}

// Helper functions for default values
fn default_max_requests() -> usize { 100 }
fn default_window_seconds() -> u64 { 60 }
fn default_true() -> bool { true }
fn default_http_methods() -> Vec<String> {
    vec!["GET".to_string(), "POST".to_string(), "PUT".to_string(), "DELETE".to_string()]
}
fn default_http_headers() -> Vec<String> {
    vec!["Content-Type".to_string(), "Authorization".to_string()]
}
fn default_metrics_port() -> u16 { 9090 }
fn default_metrics_path() -> String { "/metrics".to_string() }
fn default_health_port() -> u16 { 8080 }
fn default_health_path() -> String { "/health".to_string() }

impl NetworkConfig {
    /// Tạo mới một đối tượng NetworkConfig mặc định
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Kiểm tra tính hợp lệ của cấu hình
    pub fn validate(&self) -> Result<(), ConfigError> {
        debug!("Validating network configuration");
        
        // Validate core config
        self.core.validate()?;
        
        // Validate plugin configs
        self.plugins.redis.validate()?;
        self.plugins.webrtc.validate()?;
        self.plugins.ipfs.validate()?;
        self.plugins.wasm.validate()?;
        
        debug!("Network configuration is valid");
        Ok(())
    }
    
    /// Đọc cấu hình từ file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let loader = ConfigLoader::new();
        loader.load_from_file(path)
    }
    
    /// Tạo cấu hình từ biến môi trường
    pub fn from_env() -> Result<Self, ConfigError> {
        let loader = ConfigLoader::new();
        loader.load_from_env()
    }
    
    /// Tải cấu hình từ file - Hỗ trợ tương thích với code cũ
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        debug!("Loading configuration using legacy load() method");
        let loader = ConfigLoader::new();
        loader.load_all(path.as_ref().to_str().unwrap_or("config/network.json"))
    }
    
    /// Kiểm tra xung đột port
    pub fn check_port_conflicts(&self) -> Result<(), ConfigError> {
        debug!("Checking port conflicts");
        
        // Danh sách các port đang sử dụng và mục đích
        let mut used_ports = Vec::new();
        
        // Core port
        if self.core.port > 0 {
            used_ports.push((self.core.port, "Main API".to_string()));
        }
        
        // gRPC port
        if self.plugins.grpc.port > 0 {
            used_ports.push((self.plugins.grpc.port, "gRPC".to_string()));
        }
        
        // Libp2p port
        if self.plugins.libp2p.port > 0 {
            used_ports.push((self.plugins.libp2p.port, "Libp2p".to_string()));
        }
        
        // Metrics port
        if self.monitoring.enable_metrics && self.monitoring.metrics_port > 0 {
            used_ports.push((self.monitoring.metrics_port, "Metrics".to_string()));
        }
        
        // Health port
        if self.monitoring.enable_health_checks && self.monitoring.health_port > 0 {
            used_ports.push((self.monitoring.health_port, "Health".to_string()));
        }
        
        // Kiểm tra xung đột
        let mut conflicts = Vec::new();
        for i in 0..used_ports.len() {
            for j in i+1..used_ports.len() {
                if used_ports[i].0 == used_ports[j].0 {
                    conflicts.push((used_ports[i].0, used_ports[i].1.clone(), used_ports[j].1.clone()));
                }
            }
        }
        
        if !conflicts.is_empty() {
            let mut error_msg = String::from("Port conflicts detected:");
            for (port, service1, service2) in conflicts {
                error_msg.push_str(&format!("\n  - Port {} is used by both {} and {}", port, service1, service2));
            }
            return Err(ConfigError::InvalidConfig(error_msg));
        }
        
        debug!("No port conflicts found");
        Ok(())
    }
}

// Export các types quan trọng nhất để sử dụng trực tiếp từ module config
pub use crate::config::types::{
    RedisConfig, IpfsConfig, WasmConfig, WebrtcConfig, 
    GrpcConfig, Libp2pConfig
}; 