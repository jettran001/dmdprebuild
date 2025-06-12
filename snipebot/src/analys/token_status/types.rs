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
    
    /// Tạo TokenStatus từ ContractInfo
    pub fn from_contract_info(
        contract_info: &ContractInfo,
        liquidity_events: &[LiquidityEvent],
    ) -> Self {
        let mut status = Self::new();
        
        // Nếu có source code thì verified
        status.contract_verified = contract_info.is_verified;
        
        // Default audit score nếu có source verified
        if contract_info.is_verified {
            status.audit_score = 60.0;
        }
        
        // Kiểm tra thanh khoản nếu có dữ liệu
        if !liquidity_events.is_empty() {
            // Tính tổng thanh khoản
            let total_liquidity: f64 = liquidity_events.iter()
                .filter(|e| e.event_type == LiquidityEventType::Added)
                .map(|e| e.amount_usd)
                .sum();
                
            let removed_liquidity: f64 = liquidity_events.iter()
                .filter(|e| e.event_type == LiquidityEventType::Removed)
                .map(|e| e.amount_usd)
                .sum();
                
            status.liquidity_pool = total_liquidity - removed_liquidity;
            status.low_liquidity = status.liquidity_pool < LIQUIDITY_DANGEROUS;
        }
        
        status
    }
    
    /// Phân tích quyền hạn của owner
    pub fn analyze_owner_privileges(&mut self, contract_info: &ContractInfo) -> OwnerAnalysis {
        let mut analysis = OwnerAnalysis {
            current_owner: contract_info.owner_address.clone(),
            is_ownership_renounced: false,
            has_multisig: false,
            is_proxy: false,
            can_retrieve_ownership: false,
            has_mint_authority: false,
            has_burn_authority: false,
            has_pause_authority: false,
        };
        
        // Nếu có source code, phân tích các hàm liên quan đến owner
        if let Some(source_code) = &contract_info.source_code {
            // Kiểm tra renounced ownership
            analysis.is_ownership_renounced = source_code.contains("_OWNER = address(0)") || 
                                             source_code.contains("renounceOwnership") && 
                                             contract_info.owner_address.as_ref().map_or(false, |addr| 
                                                addr == "0x0000000000000000000000000000000000000000");
            
            // Kiểm tra multisig
            analysis.has_multisig = source_code.contains("multiSig") || 
                                   source_code.contains("multisig") || 
                                   source_code.contains("requiredSignatures");
            
            // Kiểm tra proxy
            analysis.is_proxy = source_code.contains("delegatecall") ||
                               source_code.contains("upgradeability") || 
                               source_code.contains("TransparentUpgradeableProxy") ||
                               source_code.contains("ERC1967Proxy");
                               
            // Kiểm tra retrieve ownership
            analysis.can_retrieve_ownership = source_code.contains("transferOwnership") && 
                                             source_code.contains("onlyOwner") &&
                                             !analysis.is_ownership_renounced;
                                             
            // Kiểm tra quyền mint
            analysis.has_mint_authority = source_code.contains("function mint") || 
                                         source_code.contains("_mint");
                                         
            // Kiểm tra quyền burn
            analysis.has_burn_authority = source_code.contains("function burn") || 
                                         source_code.contains("_burn");
                                         
            // Kiểm tra quyền pause
            analysis.has_pause_authority = source_code.contains("function pause") || 
                                          source_code.contains("whenNotPaused");
            
            // Cập nhật thông tin trong TokenStatus
            self.pausable = analysis.has_pause_authority;
            self.hidden_ownership = analysis.is_proxy && !contract_info.is_verified;
            self.mint_infinite = analysis.has_mint_authority;
            self.fake_renounced_ownership = analysis.is_ownership_renounced && analysis.can_retrieve_ownership;
        }
        
        analysis
    }
    
    /// Phát hiện các hàm nguy hiểm trong contract
    pub fn detect_dangerous_functions(&self, contract_info: &ContractInfo) -> Vec<String> {
        let mut dangerous_functions = Vec::new();
        
        if let Some(source_code) = &contract_info.source_code {
            // Tìm kiếm các hàm nguy hiểm
            
            // Hàm mint không giới hạn
            if source_code.contains("function mint") && !source_code.contains("maxSupply") {
                dangerous_functions.push("Mint không giới hạn".to_string());
            }
            
            // Hàm burn token của người dùng
            if source_code.contains("function burnFrom") {
                dangerous_functions.push("burnFrom (có thể đốt token của người khác)".to_string());
            }
            
            // Selfdestruct
            if source_code.contains("selfdestruct") || source_code.contains("suicide") {
                dangerous_functions.push("selfdestruct (có thể xóa contract)".to_string());
            }
            
            // Blacklist
            if source_code.contains("blacklist") || source_code.contains("_isBlacklisted") {
                dangerous_functions.push("blacklist (chặn người dùng)".to_string());
                self.blacklist = true;
            }
            
            // Lock transfer
            if (source_code.contains("lockTransfer") || source_code.contains("disableTrading")) &&
               source_code.contains("onlyOwner") {
                dangerous_functions.push("lockTransfer (khóa giao dịch)".to_string());
                self.lock_sell = true;
            }
            
            // Các hàm tax đáng ngờ có thể thay đổi
            if (source_code.contains("setTaxFee") || 
                source_code.contains("updateFee") ||
                source_code.contains("setBuyTax") ||
                source_code.contains("setSellTax")) &&
               source_code.contains("onlyOwner") {
                dangerous_functions.push("Thay đổi thuế (tax fee)".to_string());
            }
            
            // Exclude from fee
            if source_code.contains("excludeFromFee") && source_code.contains("onlyOwner") {
                dangerous_functions.push("excludeFromFee (miễn thuế)".to_string());
            }
            
            // Tax ẩn hoặc bẫy
            if source_code.contains("_previousTaxFee") || 
               source_code.contains("_maxTxAmount") ||
               source_code.contains("maxSellTransactionAmount") {
                dangerous_functions.push("Giới hạn giao dịch hoặc thuế ẩn".to_string());
            }
        }
        
        dangerous_functions
    }
    
    /// Kiểm tra xem token có phải là proxy contract hay không
    pub fn is_proxy_contract(&self) -> bool {
        // Dựa vào thông tin đã có trong struct
        self.hidden_ownership
    }
    
    /// Kiểm tra xem contract có sử dụng delegatecall hay không
    pub fn has_external_delegatecall(&self, contract_info: &ContractInfo) -> bool {
        // Kiểm tra source code nếu có
        if let Some(source_code) = &contract_info.source_code {
            source_code.contains("delegatecall")
        } else {
            false
        }
    }
    
    /// Phân tích bytecode của contract
    pub fn analyze_bytecode(&self, contract_info: &ContractInfo) -> BytecodeAnalysis {
        let mut analysis = BytecodeAnalysis {
            is_consistent: true,
            dangerous_functions: Vec::new(),
            has_external_call: false,
            has_delegatecall: false,
            has_selfdestruct: false,
        };
        
        // Phân tích bytecode nếu có
        if let Some(bytecode) = &contract_info.bytecode {
            // Kiểm tra delegatecall opcode
            if bytecode.contains("0xf4") { // Delegatecall opcode
                analysis.has_delegatecall = true;
                analysis.dangerous_functions.push("delegatecall".to_string());
            }
            
            // Kiểm tra selfdestruct opcode
            if bytecode.contains("0xff") { // Selfdestruct opcode
                analysis.has_selfdestruct = true;
                analysis.dangerous_functions.push("selfdestruct".to_string());
            }
            
            // Kiểm tra external call opcodes
            if bytecode.contains("0xf1") || bytecode.contains("0xf2") { // Call opcodes
                analysis.has_external_call = true;
            }
        }
        
        // Nếu có source code, so sánh với bytecode để kiểm tra nhất quán
        if let (Some(source_code), Some(_)) = (&contract_info.source_code, &contract_info.bytecode) {
            // Đơn giản hóa: kiểm tra nếu source code có chứa các hàm nguy hiểm
            // mà không có trong dangerous_functions
            let source_has_selfdestruct = source_code.contains("selfdestruct") || source_code.contains("suicide");
            let source_has_delegatecall = source_code.contains("delegatecall");
            
            if (source_has_selfdestruct && !analysis.has_selfdestruct) ||
               (source_has_delegatecall && !analysis.has_delegatecall) {
                analysis.is_consistent = false;
            }
        }
        
        analysis
    }
    
    /// Kiểm tra xem contract có giới hạn max tx hoặc max wallet hay không
    pub fn has_max_tx_or_wallet_limit(&self, contract_info: &ContractInfo) -> bool {
        if let Some(source_code) = &contract_info.source_code {
            source_code.contains("maxTxAmount") || 
            source_code.contains("_maxTxAmount") || 
            source_code.contains("maxWalletSize") ||
            source_code.contains("_maxWalletSize")
        } else {
            false
        }
    }
    
    /// Kiểm tra xem contract có cooling down period hay không
    pub fn has_trading_cooldown(&self, contract_info: &ContractInfo) -> bool {
        if let Some(source_code) = &contract_info.source_code {
            source_code.contains("tradingCooldown") || 
            source_code.contains("_cooldownTimerInterval") || 
            source_code.contains("cooldownTime")
        } else {
            false
        }
    }
    
    /// Phân tích code không qua API (dựa vào source code offline)
    pub fn analyze_code_without_api(&mut self, contract_info: &ContractInfo) {
        if let Some(source_code) = &contract_info.source_code {
            // Phân tích source code
            self.suspicious_transfer_functions = source_code.contains("_transfer") && 
                (source_code.contains("fee") || source_code.contains("tax"));
            
            self.pausable = source_code.contains("pause");
            self.mint_infinite = source_code.contains("mint") && !source_code.contains("maxSupply");
            self.lock_sell = source_code.contains("lockSell") || source_code.contains("disableTrading");
            self.blacklist = source_code.contains("blacklist");
        }
    }
    
    /// Kiểm tra xem quyền sở hữu đã được từ bỏ hay chưa
    pub fn is_ownership_renounced(&self, contract_info: &ContractInfo) -> bool {
        if let Some(source_code) = &contract_info.source_code {
            // Kiểm tra renounceOwnership
            if source_code.contains("renounceOwnership") {
                if let Some(owner_address) = &contract_info.owner_address {
                    return owner_address == "0x0000000000000000000000000000000000000000";
                }
            }
        }
        self.fake_renounced_ownership // Trả về giá trị đã đánh giá trước đó
    }
    
    /// Phát hiện fee/tax ẩn
    pub fn has_hidden_fees(&self, contract_info: &ContractInfo) -> bool {
        if let Some(source_code) = &contract_info.source_code {
            // Tìm các pattern liên quan đến fee ẩn
            let obvious_fee = source_code.contains("fee") || 
                             source_code.contains("tax") ||
                             source_code.contains("Tax");
                             
            let hidden_mechanism = source_code.contains("_transfer") && 
                                  !obvious_fee && 
                                  (source_code.contains("amount") && 
                                   source_code.contains("sub") || 
                                   source_code.contains("div"));
            
            hidden_mechanism
        } else {
            false
        }
    }
    
    /// Phát hiện bất thường trong sự kiện thanh khoản
    pub fn abnormal_liquidity_events(&self, events: &[LiquidityEvent]) -> bool {
        if events.len() < 2 {
            return false;
        }
        
        // Tính tỷ lệ add/remove
        let add_count = events.iter()
            .filter(|e| e.event_type == LiquidityEventType::Added || e.event_type == LiquidityEventType::AddLiquidity)
            .count();
            
        let remove_count = events.iter()
            .filter(|e| e.event_type == LiquidityEventType::Removed || e.event_type == LiquidityEventType::RemoveLiquidity)
            .count();
            
        // Nếu số lượng remove gần bằng hoặc lớn hơn add, đó là dấu hiệu bất thường
        if remove_count > 0 && (add_count as f64) / (remove_count as f64) < 1.5 {
            return true;
        }
        
        // Kiểm tra thời gian giữa add và remove
        let mut sorted_events = events.to_vec();
        sorted_events.sort_by_key(|e| e.timestamp);
        
        for i in 0..sorted_events.len() - 1 {
            if (sorted_events[i].event_type == LiquidityEventType::Added || 
                sorted_events[i].event_type == LiquidityEventType::AddLiquidity) && 
               (sorted_events[i+1].event_type == LiquidityEventType::Removed || 
                sorted_events[i+1].event_type == LiquidityEventType::RemoveLiquidity) {
                // Nếu remove xảy ra ngay sau add (trong vòng 1 giờ)
                if sorted_events[i+1].timestamp - sorted_events[i].timestamp < 3600 {
                    return true;
                }
            }
        }
        
        false
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
    /// Địa chỉ token
    pub token_address: String,
    /// Giá trị USD của sự kiện
    pub amount_usd: f64,
    /// Hash của giao dịch
    pub transaction_hash: String,
    /// Số block
    pub block_number: u64,
    /// Thời gian xảy ra sự kiện (timestamp)
    pub timestamp: u64,
    /// Địa chỉ nguồn
    pub from_address: String,
    /// Địa chỉ đích
    pub to_address: String,
    /// Tên DEX
    pub dex_name: String,
    /// Số lượng token
    pub token_amount: String,
    /// Số lượng tiền (ETH, BNB, etc.)
    pub base_amount: String,
}

/// Loại sự kiện thanh khoản
#[derive(Debug, Clone, PartialEq)]
pub enum LiquidityEventType {
    Added,
    Removed,
    Transferred,
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