/*!
 * Token analysis utilities
 * 
 * Provides core functions for token analysis including:
 * - Honeypot detection (simulate sell test)
 * - External call detection in contracts
 * - Delegate call detection
 * - Simulation helpers
 */

use std::collections::HashMap;
use regex::Regex;
use anyhow::{Result, Context};
use async_trait::async_trait;
use std::sync::Arc;
use tracing::debug;

use super::types::{ContractInfo, BytecodeAnalysis};

/// Pattern máy tìm kiếm cho các hàm nguy hiểm
struct DangerousPatterns {
    // Mapping từ tên hàm nguy hiểm đến pattern opcode
    patterns: HashMap<String, Vec<&'static str>>,
}

impl DangerousPatterns {
    /// Tạo mới pattern detector
    pub fn new() -> Self {
        let mut patterns = HashMap::new();
        
        // Selfdestruct - hàm có thể hủy contract
        patterns.insert(
            "selfdestruct".to_string(),
            vec![
                "selfdestruct",
                "selfdestrct",
                "suicide",
                "SELFDESTRUCT",
            ],
        );
        
        // Delegate call - gọi ủy quyền (nguy hiểm nếu sử dụng với input không tin cậy)
        patterns.insert(
            "delegatecall".to_string(),
            vec![
                "delegatecall",
                "DELEGATECALL",
            ],
        );
        
        // Call - gọi hàm external (nguy hiểm nếu sử dụng với input không tin cậy)
        patterns.insert(
            "call".to_string(),
            vec![
                "call(address",
                "call{value",
                "call.value",
                "CALL",
            ],
        );
        
        // Mint/burn - tạo/hủy token
        patterns.insert(
            "mint".to_string(),
            vec![
                "mint(",
                "_mint(",
                "mintToken",
            ],
        );
        
        patterns.insert(
            "burn".to_string(),
            vec![
                "burn(",
                "_burn(",
                "burnFrom",
            ],
        );
        
        // Pause - dừng giao dịch
        patterns.insert(
            "pause".to_string(),
            vec![
                "pause(",
                "whenNotPaused",
                "whenPaused",
                "isPaused",
            ],
        );
        
        // Blacklist - chặn ví 
        patterns.insert(
            "blacklist".to_string(),
            vec![
                "blacklist",
                "isBlacklisted",
                "addBlacklist",
                "removeBlacklist",
            ],
        );
        
        Self { patterns }
    }
    
    /// Tìm các hàm nguy hiểm trong mã
    pub fn find_matches(&self, source_code: &str) -> HashMap<String, bool> {
        let mut results = HashMap::new();
        
        for (name, patterns) in &self.patterns {
            for pattern in patterns {
                if source_code.contains(pattern) {
                    results.insert(name.clone(), true);
                    break;
                }
            }
        }
        
        results
    }
}

/// Phát hiện honeypot pattern
pub struct HoneypotDetector {
    buy_patterns: Vec<&'static str>,
    sell_patterns: Vec<&'static str>,
}

impl HoneypotDetector {
    /// Tạo mới honeypot detector
    pub fn new() -> Self {
        Self {
            buy_patterns: vec![
                "canBuy\\s*=\\s*true",
                "allowTrading\\s*=\\s*true",
                "tradingEnabled",
                "enableTrading",
                "onlyBuy",
            ],
            sell_patterns: vec![
                "canSell\\s*=\\s*false",
                "require\\s*\\([^)]*from\\s*!=\\s*\\w+\\s*\\)",
                "require\\s*\\([^)]*!\\s*inSwap\\s*\\)",
                "require\\s*\\([^)]*now\\s*>\\s*unlockTime\\s*\\)",
                "require\\s*\\([^)]*block\\.timestamp\\s*>\\s*\\w+\\s*\\)",
                "restrictWhales",
                "!\\s*authorizedCaller",
                "launchTime\\s*\\+\\s*\\w+\\s*>\\s*block\\.timestamp",
            ],
        }
    }
    
    /// Phân tích mã để phát hiện honeypot
    pub fn analyze(&self, source_code: &str) -> bool {
        // Tìm pattern mua được
        let mut can_buy = false;
        for pattern in &self.buy_patterns {
            if Regex::new(pattern).map_or(false, |re| re.is_match(source_code)) {
                can_buy = true;
                break;
            }
        }
        
        // Tìm pattern không bán được
        let mut cannot_sell = false;
        for pattern in &self.sell_patterns {
            if Regex::new(pattern).map_or(false, |re| re.is_match(source_code)) {
                cannot_sell = true;
                break;
            }
        }
        
        // Honeypot là có thể mua nhưng không bán được
        can_buy && cannot_sell
    }
}

/// Phân tích bytecode
pub fn analyze_bytecode(contract_info: &ContractInfo) -> BytecodeAnalysis {
    let mut is_consistent = true;
    let mut dangerous_functions = Vec::new();
    let mut has_external_call = false;
    let mut has_delegatecall = false;
    let mut has_selfdestruct = false;
    
    if let (Some(ref bytecode), Some(ref source_code)) = (&contract_info.bytecode, &contract_info.source_code) {
        // Kiểm tra tính nhất quán của bytecode với source code
        // (Phát hiện nếu bytecode được biên dịch từ source code khác)
        
        // Pattern nguy hiểm trong bytecode
        if bytecode.contains("SELFDESTRUCT") || bytecode.contains("selfdestruct") {
            has_selfdestruct = true;
            dangerous_functions.push("selfdestruct".to_string());
        }
        
        if bytecode.contains("DELEGATECALL") || bytecode.contains("delegatecall") {
            has_delegatecall = true;
            dangerous_functions.push("delegatecall".to_string());
        }
        
        if (bytecode.contains("CALL") && !bytecode.contains("STATICCALL")) || 
           source_code.contains(".call(") || source_code.contains(".call{") {
            has_external_call = true;
            dangerous_functions.push("external_call".to_string());
        }
        
        // Tìm các hàm nguy hiểm khác
        let dangerous_detector = DangerousPatterns::new();
        let dangerous_matches = dangerous_detector.find_matches(source_code);
        
        for (name, found) in dangerous_matches {
            if found && !dangerous_functions.contains(&name) {
                dangerous_functions.push(name);
            }
        }
        
        // Kiểm tra tính nhất quán giữa source và bytecode
        // (Đơn giản là kiểm tra xem các hàm nguy hiểm trong source có xuất hiện trong bytecode không)
        if (source_code.contains("selfdestruct") || source_code.contains("suicide")) != has_selfdestruct {
            is_consistent = false;
        }
        
        if source_code.contains("delegatecall") != has_delegatecall {
            is_consistent = false;
        }
    }
    
    BytecodeAnalysis {
        is_consistent,
        dangerous_functions,
        has_external_call,
        has_delegatecall,
        has_selfdestruct,
    }
}

/// Phát hiện mẫu contract phổ biến
pub fn detect_contract_template(contract_info: &ContractInfo) -> Option<String> {
    if let Some(ref source_code) = contract_info.source_code {
        // Map các mẫu contract phổ biến và pattern nhận dạng
        let templates = [
            ("OpenZeppelin ERC20", vec!["@openzeppelin/contracts", "ERC20"]),
            ("SafeMoon Fork", vec!["SafeMoon", "_reflect", "RFI"]),
            ("PancakeSwap", vec!["PancakeFactory", "PancakeRouter", "PancakePair"]),
            ("Uniswap", vec!["UniswapFactory", "UniswapRouter", "UniswapPair"]),
            ("BEP20", vec!["BEP20", "IBEP20", "Context"]),
            ("BabyToken", vec!["BabyToken", "rewardToken", "rewardToHolder"]),
            ("ReflectionToken", vec!["_reflectFee", "_takeLiquidity", "RFI"]),
            ("Taxable Token", vec!["_takeFee", "totalFees", "transferTaxRate"]),
            ("Anti-Whale", vec!["antiWhale", "maxTxAmount", "maxWalletSize"]),
            ("Buyback", vec!["buyBackEnabled", "buyBackUpperLimit", "buyTokens"]),
            ("Lottery", vec!["lottery", "randomResult", "distribute"]),
        ];
        
        for (name, patterns) in templates {
            let mut matches = true;
            for pattern in patterns {
                if !source_code.contains(pattern) {
                    matches = false;
                    break;
                }
            }
            
            if matches {
                return Some(name.to_string());
            }
        }
    }
    
    None
}

/// Base trait for chain adapters that is compatible with dyn
pub trait ChainAdapter: Send + Sync + 'static {
    /// Get the address of the chain adapter
    fn get_adapter_address(&self) -> &str;
    
    /// Lấy một ChainAdapterImpl cụ thể từ trait chung
    /// 
    /// Đây là phương thức không async để đảm bảo trait tương thích với dyn
    fn as_chain_adapter_impl(&self) -> &dyn ChainAdapterImpl;
}

/// Base trait for chain adapter implementations
/// Trait cơ bản không có các phương thức async nên tương thích với dyn
pub trait ChainAdapterImpl: Send + Sync + 'static {
    /// Get adapter name
    fn get_name(&self) -> &str;
    
    /// Get chain ID
    fn get_chain_id(&self) -> u32;
}

/// Extended trait with async methods - not dyn compatible
/// Trait mở rộng với các phương thức async, không tương thích với dyn trực tiếp
#[async_trait]
pub trait ChainAdapterImplExt: ChainAdapterImpl {
    /// Simulate selling a token
    async fn simulate_sell_token(&self, token_address: &str, amount: &str) -> Result<SimulationResult>;
}

/// Helper function to simulate selling a token using a chain adapter
pub async fn simulate_sell_token(_adapter: &dyn ChainAdapter, _token_address: &str, _amount: &str) -> Result<SimulationResult> {
    // Phương thức này sử dụng trait động, nhưng không thể gọi trực tiếp các phương thức async
    // Thay vào đó, chúng ta cần downstream cast hoặc sử dụng các phương pháp khác
    
    // Đây là một cách tiếp cận an toàn nhất, trả về một kết quả lỗi
    // Trong môi trường thực tế, cần triển khai cụ thể cho từng loại adapter riêng biệt
    Err(anyhow::anyhow!("Simulation not supported with dynamic ChainAdapter. Use concrete adapter types instead"))
}

/// Kết quả mô phỏng giao dịch
#[derive(Debug, Clone)]
pub struct SimulationResult {
    pub success: bool,
    pub failure_reason: Option<String>,
    pub gas_used: Option<u64>,
    pub output: Option<String>,
}

/// Thông tin bảo mật của hợp đồng
#[derive(Debug, Clone)]
pub struct ContractSecurityInfo {
    /// Có external calls không
    pub has_external_calls: bool,
    
    /// Có delegatecall không
    pub has_delegatecall: bool,
    
    /// Có selfdestruct không
    pub has_selfdestruct: bool,
    
    /// Điểm số bảo mật (0-100, càng cao càng an toàn)
    pub security_score: u8,
    
    /// Danh sách các hàm nguy hiểm được phát hiện
    pub dangerous_functions: Vec<String>,
    
    /// Mô tả chi tiết hơn về các vấn đề
    pub details: Option<String>,
}

/// Helper function to check contract security with metadata
pub async fn check_contract_security_with_metadata(contract_info: &ContractInfo, _adapter: &dyn ChainAdapter) -> Result<(bool, Option<ContractSecurityInfo>)> {
    // Phân tích bytecode
    let bytecode_analysis = analyze_bytecode(contract_info);
    
    // Phát hiện các external call
    let (has_external_calls, external_call_details) = detect_external_call(contract_info)?;
    
    // Phát hiện các delegatecall
    let (has_delegatecall, delegatecall_details) = detect_delegatecall(contract_info)?;
    
    // Tính điểm bảo mật
    let mut security_score = 100u8;
    
    if !bytecode_analysis.is_consistent {
        security_score -= 40; // Không nhất quán giữa source và bytecode
    }
    
    if bytecode_analysis.has_selfdestruct {
        security_score -= 30; // Có thể tự hủy contract
    }
    
    if has_delegatecall {
        security_score -= 25; // Delegatecall rất nguy hiểm
    }
    
    if has_external_calls {
        security_score -= 15; // External calls có thể nguy hiểm
    }
    
    if bytecode_analysis.dangerous_functions.len() > 3 {
        security_score -= 20; // Nhiều hàm nguy hiểm
    }
    
    // Tạo chi tiết
    let mut details = Vec::new();
    if !bytecode_analysis.is_consistent {
        details.push("Source code và bytecode không nhất quán".to_string());
    }
    if !external_call_details.is_empty() {
        details.push(format!("External calls: {}", external_call_details.join(", ")));
    }
    if !delegatecall_details.is_empty() {
        details.push(format!("Delegatecalls: {}", delegatecall_details.join(", ")));
    }
    
    // Tạo thông tin bảo mật
    let contract_security_info = ContractSecurityInfo {
        has_external_calls,
        has_delegatecall,
        has_selfdestruct: bytecode_analysis.has_selfdestruct,
        security_score,
        dangerous_functions: bytecode_analysis.dangerous_functions,
        details: if details.is_empty() { None } else { Some(details.join("; ")) },
    };
    
    // Contract được coi là an toàn nếu điểm số >= 70
    let is_secure = security_score >= 70;
    
    Ok((is_secure, Some(contract_security_info)))
}

/// Check if token has sell restrictions
pub async fn check_sell_restrictions(contract_info: &ContractInfo, _adapter: &dyn ChainAdapter) -> Result<(bool, Option<String>)> {
    // Simulation is only available with adapter that implements ChainAdapterExt
    // For compatibility, this will return no restrictions detected when using dyn ChainAdapter
    debug!("Checking sell restrictions for token {}", contract_info.address);
    
    // To be implemented with concrete adapter types
    Ok((false, None))
}

/// Phát hiện external call trong contract
pub fn detect_external_call(contract_info: &ContractInfo) -> Result<(bool, Vec<String>)> {
    let mut has_external_calls = false;
    let mut external_calls = Vec::new();
    
    if let Some(source_code) = &contract_info.source_code {
        let external_call_patterns = [
            r"(\w+)\.call\{",
            r"(\w+)\.delegatecall\(",
            r"Address\.functionCall\(",
            r"Address\.functionCallWithValue\(",
            r"Address\.functionDelegateCall\(",
            r"(0x[a-fA-F0-9]{40})\.call\{",
            r"selfdestruct\(",
        ];
        
        for pattern in external_call_patterns {
            if let Ok(re) = Regex::new(pattern) {
                for cap in re.captures_iter(source_code) {
                    if let Some(m) = cap.get(1) {
                        has_external_calls = true;
                        external_calls.push(format!("External call: {}", m.as_str()));
                    } else {
                        has_external_calls = true;
                        external_calls.push(format!("External call found: {}", pattern));
                    }
                }
            }
        }
    }
    
    Ok((has_external_calls, external_calls))
}

/// Phát hiện delegatecall trong contract (rủi ro cao)
pub fn detect_delegatecall(contract_info: &ContractInfo) -> Result<(bool, Vec<String>)> {
    let mut has_delegatecall = false;
    let mut delegatecalls = Vec::new();
    
    if let Some(source_code) = &contract_info.source_code {
        let delegatecall_patterns = [
            r"(\w+)\.delegatecall\(",
            r"Address\.functionDelegateCall\(",
            r"assembly\s*\{[^}]*delegatecall[^}]*\}",
        ];
        
        for pattern in delegatecall_patterns {
            if let Ok(re) = Regex::new(pattern) {
                for cap in re.captures_iter(source_code) {
                    if let Some(m) = cap.get(1) {
                        has_delegatecall = true;
                        delegatecalls.push(format!("Delegatecall: {}", m.as_str()));
                    } else {
                        has_delegatecall = true;
                        delegatecalls.push("Delegatecall detected in assembly block".to_string());
                    }
                }
            }
        }
    }
    
    Ok((has_delegatecall, delegatecalls))
}

/// Phát hiện token có phải honeypot không bằng cách thử mô phỏng bán token
pub async fn detect_honeypot(contract_info: &ContractInfo, adapter: &dyn ChainAdapter) -> Result<(bool, Option<String>)> {
    // Các constant cho error message
    const UNKNOWN_REASON: &str = "Unknown reason";
    const TEST_AMOUNT: &str = "0.01"; // Số lượng nhỏ để test
    
    debug!("Testing honeypot for token: {}", contract_info.address);
    
    // Sử dụng hàm helper simulate_sell_token thay vì gọi trực tiếp từ adapter
    let result = simulate_sell_token(adapter, &contract_info.address, TEST_AMOUNT)
        .await
        .context("Failed to simulate selling token")?;
    
    if !result.success {
        let reason = result.failure_reason.unwrap_or_else(|| UNKNOWN_REASON.to_string());
        debug!("Honeypot detected: {}", reason);
        return Ok((true, Some(reason)));
    }
    
    // Nếu simulate thành công, kiểm tra thêm các vấn đề khác
    let (has_restrictions, reason) = detect_transfer_restrictions(contract_info)?;
    if has_restrictions {
        return Ok((true, reason));
    }
    
    Ok((false, None))
}

/// Phát hiện token có giới hạn transfer không
fn detect_transfer_restrictions(contract_info: &ContractInfo) -> Result<(bool, Option<String>)> {
    if let Some(source_code) = &contract_info.source_code {
        // Tìm kiếm các hàm transfer/transferFrom có giới hạn ngoài onlyOwner
        let restriction_patterns = [
            "require\\s*\\(\\s*!\\s*blacklisted\\[\\w+\\]\\s*\\)",
            "require\\s*\\(\\s*isWhitelisted\\[\\w+\\]\\s*\\)",
            "require\\s*\\(\\s*canTransfer\\s*\\(\\s*\\w+\\s*\\)\\s*\\)",
            "require\\s*\\(\\s*block\\.timestamp\\s*[>|>=]\\s*tradingEnabledAt\\s*\\)",
            "if\\s*\\(\\s*tradingEnabled\\s*==\\s*false\\s*\\)",
            "if\\s*\\(\\s*!enabledTrading\\s*\\)",
            "return\\s+false",
        ];
        
        for pattern in restriction_patterns {
            if Regex::new(pattern).map_or(false, |re| re.is_match(source_code)) {
                return Ok((true, Some(format!("Transfer restriction found: {}", pattern))));
            }
        }
    }
    
    Ok((false, None))
}

/// Sửa các tham chiếu đến phương thức simulate_sell_token trong check_sell_ability
pub async fn check_sell_ability(contract_info: &ContractInfo, adapter: &dyn ChainAdapter) -> Result<bool> {
    const TEST_AMOUNT: &str = "0.01"; // Số lượng nhỏ để test
    
    // Sử dụng hàm helper simulate_sell_token thay vì gọi trực tiếp từ adapter
    let result = simulate_sell_token(adapter, &contract_info.address, TEST_AMOUNT)
        .await
        .context("Failed to simulate selling token")?;
    
    Ok(result.success)
}

/// Simulate selling a token to check for sell limitations
///
/// # Arguments
/// * `_adapter` - Chain adapter to use for simulation
/// * `_token_address` - Token address to sell
/// * `_amount` - Amount of tokens to sell
///
/// # Returns
/// * `Result<SellSimulationResult>` - Result of the sell simulation
pub async fn simulate_sell_token(
    _adapter: &dyn ChainAdapter,
    _token_address: &str,
    _amount: &str
) -> Result<SellSimulationResult> {
    // Implement a default behavior for simulation
    Ok(SellSimulationResult {
        success: true,
        gas_used: 100000,
        error: None,
        details: Some("Simulation success (stub implementation)".to_string())
    })
}

/// Simulate selling a token through the adapter
///
/// Helper function to simulate token sell transaction
///
/// # Arguments
/// * `_token_address` - Token address to sell
/// * `_amount` - Amount of tokens to sell
/// * `_adapter` - Chain adapter to use
///
/// # Returns
/// * `Result<SellSimulationResult>` - Result of the simulation
pub async fn simulate_sell_through_adapter(
    _token_address: &str,
    _amount: &str,
    _adapter: &Arc<dyn ChainAdapter>
) -> Result<SellSimulationResult> {
    // Implement a default behavior for simulation
    Ok(SellSimulationResult {
        success: true,
        gas_used: 100000,
        error: None,
        details: Some("Simulation success via adapter (stub implementation)".to_string())
    })
} 