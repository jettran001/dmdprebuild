//! Module analys
//!
//! Module này chịu trách nhiệm phân tích dữ liệu blockchain và token, bao gồm:
//! - Token analysis: Phân tích độ an toàn và trạng thái của token
//! - Mempool analysis: Phân tích giao dịch trong mempool
//! - Risk analysis: Đánh giá rủi ro giao dịch

pub mod token_status;
pub mod mempool;
pub mod risk_analyzer; 
pub mod api; // API cho mev_logic và các module khác sử dụng 

// Re-export usage example
#[cfg(test)]
pub use api::usage_example; 