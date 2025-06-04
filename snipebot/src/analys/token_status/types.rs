//! Định nghĩa các kiểu dữ liệu cho phân tích token

// Định nghĩa các ngưỡng và tiêu chí đánh giá
// Đưa các ngưỡng này vào hằng số để dễ quản lý và điều chỉnh
const AUDIT_SCORE_DANGEROUS: f64 = 60.0;
const AUDIT_SCORE_SAFE: f64 = 80.0;
const TAX_DANGEROUS: f64 = 10.0;
const TAX_MODERATE: f64 = 5.0;
const LIQUIDITY_DANGEROUS: f64 = 1000.0;
const LIQUIDITY_MODERATE: f64 = 3000.0;
const LIQUIDITY_SAFE: f64 = 10000.0;

// Import TokenIssue từ tradelogic/common/types.rs để tránh định nghĩa trùng lặp
use crate::tradelogic::common::types::TokenIssue;

#[derive(Debug, Clone)]
pub struct TokenStatus {
    /// Điểm audit, giá trị từ 0 đến 100.
    pub audit_score: f64,
    /// Thuế mua, tính theo phần trăm.
    pub tax_buy: f64,
    /// Thuế bán, tính theo phần trăm.
    pub tax_sell: f64,
    /// Giá trị thanh khoản của pool, tính bằng USD.
    pub liquidity_pool: f64,
    /// Smart contract đã được verified hay chưa.
    pub contract_verified: bool,
    /// Thanh khoản đã được khóa hay chưa.
    pub pool_locked: bool,
    /// Có mint vô hạn hay không.
    pub mint_infinite: bool,
    /// Có lock sell (không thể bán) hay không.
    pub lock_sell: bool,
    /// Có phải honeypot (mua được nhưng không bán được) hay không.
    pub honeypot: bool,
    /// Có rebate giả mạo (trả lại token giả) hay không.
    pub rebate: bool,
    /// Có hàm pause() cho phép tạm dừng giao dịch hay không.
    pub pausable: bool,
    /// Có hàm blacklistAddress() để chặn ví người dùng hay không.
    pub blacklist: bool,
    /// Chủ sở hữu ẩn danh hoặc contract proxy không rõ ràng.
    pub hidden_ownership: bool,
    /// Tỷ lệ sở hữu cao (>50% token nằm trong 1-2 ví).
    pub high_ownership_concentration: bool,
    /// Giả mạo từ bỏ quyền sở hữu nhưng vẫn giữ quyền qua backdoor.
    pub fake_renounced_ownership: bool,
    /// Thanh khoản thấp (< $1,000).
    pub low_liquidity: bool,
    /// Hàm transfer() tùy chỉnh đáng ngờ (có thể đánh cắp token).
    pub suspicious_transfer_functions: bool,
}

impl TokenStatus {
    /// Tạo mới TokenStatus với giá trị mặc định
    pub fn new() -> Self {
        Self {
            audit_score: 0.0,
            tax_buy: 0.0,
            tax_sell: 0.0,
            liquidity_pool: 0.0,
            contract_verified: false,
            pool_locked: false,
            mint_infinite: false,
            lock_sell: false,
            honeypot: false,
            rebate: false,
            pausable: false,
            blacklist: false,
            hidden_ownership: false,
            high_ownership_concentration: false,
            fake_renounced_ownership: false,
            low_liquidity: false,
            suspicious_transfer_functions: false,
        }
    }
    
    /// Tạo mới TokenStatus với các giá trị cụ thể
    pub fn with_values(
        audit_score: f64, 
        tax_buy: f64, 
        tax_sell: f64,
        liquidity_pool: f64,
        contract_verified: bool,
        pool_locked: bool,
    ) -> Self {
        Self {
            audit_score,
            tax_buy,
            tax_sell,
            liquidity_pool,
            contract_verified,
            pool_locked,
            mint_infinite: false,
            lock_sell: false,
            honeypot: false,
            rebate: false,
            pausable: false,
            blacklist: false,
            hidden_ownership: false,
            high_ownership_concentration: false,
            fake_renounced_ownership: false,
            low_liquidity: liquidity_pool < LIQUIDITY_DANGEROUS,
            suspicious_transfer_functions: false,
        }
    }
}

/// Các mức độ trạng thái của token.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenSafety {
    /// Nguy hiểm (🔴): Token có rủi ro cao, nên tránh.
    Dangerous,
    /// Trung bình (🟡): Token chấp nhận được nhưng cần thận trọng.
    Moderate,
    /// Tốt (🟢): Token an toàn, đáng tin cậy.
    Safe,
}

/// Mức độ nghiêm trọng của vấn đề
#[derive(Debug, Clone, PartialEq)]
pub enum IssueSeverity {
    Critical,
    High,
    Medium,
    Low,
}

/// Đề xuất hành động giao dịch
#[derive(Debug, Clone, PartialEq)]
pub enum TradeRecommendation {
    Avoid,
    ProceedWithCaution(String),
    SafeToProceed,
}

/// Thông tin về contract của token, được sử dụng cho phân tích nâng cao
#[derive(Debug, Clone)]
pub struct ContractInfo {
    /// Địa chỉ contract của token
    pub address: String,
    /// Chain ID của blockchain mà token được triển khai
    pub chain_id: u32,
    /// Source code của contract (nếu đã verified)
    pub source_code: Option<String>,
    /// Bytecode của contract từ blockchain
    pub bytecode: Option<String>,
    /// ABI của contract (nếu đã verified)
    pub abi: Option<String>,
    /// Có verified trên explorer không
    pub is_verified: bool,
    /// Địa chỉ owner của contract
    pub owner_address: Option<String>,
}

/// Thông tin về một sự kiện thanh khoản (add/remove liquidity)
#[derive(Debug, Clone)]
pub struct LiquidityEvent {
    /// Loại sự kiện (add/remove)
    pub event_type: LiquidityEventType,
    /// Thời gian xảy ra sự kiện (timestamp)
    pub timestamp: u64,
    /// Số lượng token
    pub token_amount: String,
    /// Số lượng tiền (ETH, BNB, etc.)
    pub base_amount: String,
    /// Địa chỉ người thực hiện
    pub executor: String,
    /// Giao dịch hash
    pub transaction_hash: String,
}

/// Loại sự kiện thanh khoản
#[derive(Debug, Clone, PartialEq)]
pub enum LiquidityEventType {
    AddLiquidity,
    RemoveLiquidity,
}

/// Thông tin từ các API phân tích token bên ngoài
#[derive(Debug, Clone)]
pub struct ExternalTokenReport {
    /// Nguồn báo cáo (GoPlus, StaySafu, TokenSniffer, v.v.)
    pub source: ExternalReportSource,
    /// Điểm rủi ro (từ 0 đến 100, càng cao càng nguy hiểm)
    pub risk_score: Option<f64>,
    /// Các cảnh báo rủi ro
    pub risk_flags: Vec<String>,
    /// Trạng thái honeypot
    pub is_honeypot: Option<bool>,
    /// Có trong blacklist không
    pub is_blacklisted: Option<bool>,
    /// Các hàm nguy hiểm được phát hiện
    pub dangerous_functions: Vec<String>,
    /// Dữ liệu báo cáo gốc (JSON string)
    pub raw_data: String,
}

/// Nguồn báo cáo phân tích token
#[derive(Debug, Clone, PartialEq)]
pub enum ExternalReportSource {
    GoPlus,
    StaySafu,
    TokenSniffer,
    RugDoc,
    Other(String),
}

/// Kết quả phân tích quyền owner của contract
#[derive(Debug, Clone)]
pub struct OwnerAnalysis {
    /// Địa chỉ owner hiện tại
    pub current_owner: Option<String>,
    /// Owner đã từ bỏ quyền chưa (renounced ownership)
    pub is_ownership_renounced: bool,
    /// Có multisig không
    pub has_multisig: bool,
    /// Có proxy không
    pub is_proxy: bool,
    /// Có thể lấy lại quyền owner sau khi từ bỏ không
    pub can_retrieve_ownership: bool,
    /// Có quyền mint không
    pub has_mint_authority: bool,
    /// Có quyền burn không
    pub has_burn_authority: bool,
    /// Có quyền pause không
    pub has_pause_authority: bool,
}

/// Kết quả phân tích bytecode
#[derive(Debug, Clone)]
pub struct BytecodeAnalysis {
    /// Source code và bytecode có khớp nhau không
    pub is_consistent: bool,
    /// Các hàm nguy hiểm được phát hiện trong bytecode
    pub dangerous_functions: Vec<String>,
    /// Có external call không
    pub has_external_call: bool,
    /// Có delegatecall không
    pub has_delegatecall: bool,
    /// Có selfdestruct không
    pub has_selfdestruct: bool,
}

/// Kết quả phân tích nâng cao
#[derive(Debug, Clone)]
pub struct AdvancedTokenAnalysis {
    /// Có cơ chế anti-bot không
    pub has_anti_bot: bool,
    /// Có cơ chế anti-whale không
    pub has_anti_whale: bool,
    /// Có giới hạn số lượng trong ví không
    pub has_wallet_limit: bool,
    /// Có hàm nguy hiểm không (mint, pause, etc.)
    pub has_dangerous_functions: bool,
    /// Có thanh khoản đủ không
    pub has_sufficient_liquidity: bool,
    /// Thanh khoản đã khóa chưa
    pub has_locked_liquidity: bool,
    /// Thời gian khóa thanh khoản (giây)
    pub liquidity_lock_time: u64,
}

/// Báo cáo đầy đủ về token
#[derive(Debug, Clone)]
pub struct TokenReport {
    /// Địa chỉ contract của token
    pub token_address: String,
    /// Chain ID của blockchain
    pub chain_id: u32,
    /// Mức độ an toàn của token
    pub token_safety: TokenSafety,
    /// Điểm rủi ro (0-100)
    pub risk_score: f64,
    /// Các vấn đề được phát hiện
    pub issues: Vec<(TokenIssue, IssueSeverity)>,
    /// Mẫu contract được phát hiện
    pub contract_template: Option<String>,
    /// Có whitelist không
    pub has_whitelist: bool,
    /// Có blacklist không
    pub has_blacklist: bool,
    /// Có giới hạn giao dịch không
    pub has_transaction_limit: bool,
    /// Có tính năng anti-whale không
    pub has_anti_whale: bool,
    /// Có thời gian chờ giữa các giao dịch không
    pub has_cooldown: bool,
    /// Owner đã từ bỏ quyền chưa
    pub is_ownership_renounced: bool,
    /// Các hàm nguy hiểm được phát hiện
    pub dangerous_functions: Vec<String>,
    /// Là proxy contract có thể upgrade không
    pub is_proxy_contract: bool,
    /// Có fees/tax ẩn không
    pub has_hidden_fees: bool,
    /// Có bất thường trong lịch sử thanh khoản không
    pub has_abnormal_liquidity: bool,
    /// Có external calls/delegatecall không
    pub has_external_calls: bool,
    /// Source code có khớp với bytecode không
    pub is_source_consistent: bool,
    /// Báo cáo từ các API bên ngoài
    pub external_reports: Vec<String>,
}

impl TokenReport {
    /// Tạo báo cáo token mới
    pub fn new(
        token_address: String,
        chain_id: u32,
        token_safety: TokenSafety,
        risk_score: f64,
    ) -> Self {
        Self {
            token_address,
            chain_id,
            token_safety,
            risk_score,
            issues: Vec::new(),
            contract_template: None,
            has_whitelist: false,
            has_blacklist: false,
            has_transaction_limit: false,
            has_anti_whale: false,
            has_cooldown: false,
            is_ownership_renounced: false,
            dangerous_functions: Vec::new(),
            is_proxy_contract: false,
            has_hidden_fees: false,
            has_abnormal_liquidity: false,
            has_external_calls: false,
            is_source_consistent: true,
            external_reports: Vec::new(),
        }
    }
} 