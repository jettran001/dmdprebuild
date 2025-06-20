//! Traits cho MEV Logic
//!
//! Module này định nghĩa các trait chính sử dụng trong MEV logic.

use std::sync::Arc;
use async_trait::async_trait;

use crate::analys::mempool::{MempoolAnalyzer, MempoolTransaction, SuspiciousPattern};
use crate::analys::token_status::TokenSafety;
use crate::chain_adapters::evm_adapter::EvmAdapter;

use super::types::{MevConfig, MevOpportunity, TraderBehaviorAnalysis};

/// Trait cho MEV strategy
#[async_trait]
pub trait MevBot {
    /// Bắt đầu theo dõi MEV
    async fn start(&self);
    
    /// Dừng theo dõi MEV
    async fn stop(&self);
    
    /// Cập nhật cấu hình
    async fn update_config(&self, config: MevConfig);
    
    /// Lấy danh sách cơ hội
    async fn get_opportunities(&self) -> Vec<MevOpportunity>;
    
    /// Thêm chain để theo dõi
    async fn add_chain(&mut self, chain_id: u32, adapter: Arc<EvmAdapter>);
    
    /// Đánh giá token mới
    async fn evaluate_new_token(&self, chain_id: u32, token_address: &str) -> Option<TokenSafety>;
    
    /// Phân tích cơ hội giao dịch từ token đã đánh giá
    async fn analyze_trading_opportunity(&self, chain_id: u32, token_address: &str, token_safety: TokenSafety) -> Option<MevOpportunity>;
    
    /// Theo dõi thanh khoản token
    async fn monitor_token_liquidity(&self, chain_id: u32, token_address: &str);
    
    /// Phát hiện mẫu giao dịch đáng ngờ
    async fn detect_suspicious_transaction_patterns(&self, chain_id: u32, transactions: &[MempoolTransaction]) -> Vec<SuspiciousPattern>;
    
    /// Tìm các giao dịch có khả năng thất bại trong mempool
    /// 
    /// Params:
    /// - chain_id: ID của blockchain
    /// - include_nonce_gaps: Có bao gồm giao dịch có nonce gap không
    /// - include_low_gas: Có bao gồm giao dịch có gas price thấp không
    /// - include_long_pending: Có bao gồm giao dịch chờ quá lâu không
    /// - min_wait_time_sec: Thời gian chờ tối thiểu để coi là lâu (giây)
    /// - limit: Số lượng giao dịch trả về tối đa
    async fn find_potential_failing_transactions(
        &self,
        chain_id: u32,
        include_nonce_gaps: bool,
        include_low_gas: bool,
        include_long_pending: bool,
        min_wait_time_sec: u64,
        limit: usize,
    ) -> Vec<(MempoolTransaction, String)>;
    
    /// Ước tính xác suất thành công của một giao dịch cụ thể
    /// 
    /// Params:
    /// - chain_id: ID của blockchain
    /// - transaction: Giao dịch cần đánh giá
    /// - include_details: Có bao gồm chi tiết lý do không
    async fn estimate_transaction_success_probability(
        &self,
        chain_id: u32,
        transaction: &MempoolTransaction,
        include_details: bool,
    ) -> (f64, Option<String>);
    
    /// Phân tích các giao dịch của trader cụ thể để dự đoán hành vi
    /// 
    /// Params:
    /// - chain_id: ID của blockchain
    /// - trader_address: Địa chỉ trader cần phân tích
    /// - time_window_sec: Khoảng thời gian phân tích (giây)
    async fn analyze_trader_behavior(
        &self,
        chain_id: u32,
        trader_address: &str,
        time_window_sec: u64,
    ) -> Option<TraderBehaviorAnalysis>;
}

/// Trait cho bridge provider
#[async_trait]
pub trait BridgeProvider: Send + Sync {
    /// Lấy chain ID nguồn
    fn source_chain_id(&self) -> u64;
    
    /// Lấy chain ID đích
    fn destination_chain_id(&self) -> u64;
    
    /// Lấy tên bridge
    fn name(&self) -> &str;
    
    /// Lấy trạng thái bridge
    async fn get_bridge_status(&self) -> Result<BridgeStatus>;
    
    /// Ước tính phí bridge
    async fn estimate_bridge_fee(&self, token_address: &str, amount: u128) -> Result<FeeEstimate>;
    
    /// Thực thi bridge transaction
    async fn execute_bridge(&self, transaction: BridgeTransaction) -> Result<String>;
} 