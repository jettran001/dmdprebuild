//! Module token_status
//!
//! Module này chịu trách nhiệm phân tích trạng thái và độ an toàn của token, bao gồm:
//! - Phân tích quyền hạn owner, chủ sở hữu, contract proxy
//! - Phân tích tax/fee, phí ẩn, thuế động
//! - Phân tích blacklist/whitelist, các cơ chế giới hạn
//! - Phân tích thanh khoản, abnormal events, rugpull
//! - Các tiện ích và công cụ để phân tích smart contract

// Định nghĩa các kiểu cơ bản cho token_status
mod types;

// Phân tích owner, renounce, backdoor, proxy
mod owner;

// Phân tích tax, dynamic tax, hidden fee
mod tax;

// Phân tích blacklist/whitelist
mod blacklist;

// Phân tích thanh khoản, rugpull, abnormal events
mod liquidity;

// Các tiện ích chung
mod utils;

// Sử dụng TokenIssue từ tradelogic/common/types.rs
pub use crate::tradelogic::common::types::TokenIssue;

// Re-export các struct và trait quan trọng để giữ nguyên API
pub use types::{
    TokenStatus, TokenSafety, IssueSeverity, TradeRecommendation,
    ContractInfo, LiquidityEvent, LiquidityEventType, ExternalTokenReport,
    ExternalReportSource, OwnerAnalysis, BytecodeAnalysis, AdvancedTokenAnalysis,
    TokenReport,
};

// Re-export các hàm phân tích chính
pub use owner::{
    is_ownership_renounced, is_proxy_contract, analyze_owner_privileges,
};

pub use tax::{
    detect_dynamic_tax, detect_hidden_fees,
};

pub use blacklist::{
    has_blacklist_or_whitelist, has_trading_cooldown, has_max_tx_or_wallet_limit,
};

pub use liquidity::{
    abnormal_liquidity_events, detect_rugpull_pattern,
};