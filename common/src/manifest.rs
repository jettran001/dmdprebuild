
//! 🧭 Entry Point: Đây là manifest chính chứa toàn bộ module của dự án.
//! Mỗi thư mục là 1 đơn vị rõ ràng: `api`, `services`, `traits`, `utils`.
//! Bot hãy bắt đầu từ đây để resolve module path chính xác.
//! Được dùng làm tài liệu tham chiếu khi import từ domain khác


/*
    common/
    ├── middleware/auth.rs          -> Middleware xác thực dùng cho API nội bộ (snipebot, wallet)
    ├── api.rs                      -> Các DTO (data transfer object) chung cho API request/response
    ├── config.rs                   -> Tải và parse config hệ thống
    ├── error.rs                    -> Định nghĩa Error dùng chung toàn hệ thống
    ├── lib.rs                      -> Các thư viện dùng chung cho toàn bộ dự án
    ├── logger.rs                   -> Thiết lập logging chuẩn (tracing)
    ├── module_map.rs               -> Module map dùng cho các module khác trong dự án
    ├── router.rs                   -> Router template dùng chung cho các service
    ├── sdk.rs                      -> SDK dùng kết nối các domain khác
   
    Mối liên kết:
    - router phụ thuộc logger, error
    - auth phụ thuộc config và error
    - api độc lập, nhưng được dùng ở nhiều nơi
*/

// src/registry/manifest.rs
    pub mod api 
    pub mod config;
    pub mod logger;
    pub mod error;
    pub mod auth;
    pub mod api;
    pub mod router;
    
    
    