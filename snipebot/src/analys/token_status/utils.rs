//! Các hàm tiện ích và công cụ phân tích chung
//!
//! Module này chứa các công cụ và tiện ích dùng chung cho việc phân tích token

use std::collections::HashMap;
use regex::Regex;

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