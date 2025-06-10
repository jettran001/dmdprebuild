//! Phân tích quyền hạn owner, ownership renounce, proxy
//!
//! Module này chứa các hàm phân tích liên quan đến:
//! - Quyền hạn của owner
//! - Kiểm tra đã renounce ownership thật sự chưa
//! - Phát hiện backdoor để lấy lại quyền
//! - Phát hiện proxy contract có thể upgrade logic

use regex::Regex;

use super::types::{OwnerAnalysis, ContractInfo};

/// Phân tích quyền hạn của owner contract
pub fn analyze_owner_privileges(contract_info: &ContractInfo) -> OwnerAnalysis {
    let mut analyzer = OwnershipAnalyzer::new();
    
    let is_ownership_renounced = if let Some(ref source_code) = contract_info.source_code {
        analyzer.is_ownership_renounced(source_code)
    } else {
        false
    };
    
    let has_ownership_backdoor = if let Some(ref source_code) = contract_info.source_code {
        analyzer.has_ownership_backdoor(source_code)
    } else {
        false
    };
    
    let has_multisig = if let Some(ref source_code) = contract_info.source_code {
        analyzer.has_multisig(source_code)
    } else {
        false
    };
    
    let is_proxy = is_proxy_contract(contract_info);
    
    // Phân tích các quyền hạn khác
    let (has_mint_authority, has_burn_authority, has_pause_authority) = 
        analyze_other_privileges(contract_info);
    
    OwnerAnalysis {
        current_owner: contract_info.owner_address.clone(),
        is_ownership_renounced,
        has_multisig,
        is_proxy,
        can_retrieve_ownership: has_ownership_backdoor,
        has_mint_authority,
        has_burn_authority,
        has_pause_authority,
    }
}

/// Kiểm tra xem quyền sở hữu có được từ bỏ thực sự không
pub fn is_ownership_renounced(contract_info: &ContractInfo) -> bool {
    let analyzer = OwnershipAnalyzer::new();
    
    if let Some(ref source_code) = contract_info.source_code {
        analyzer.is_ownership_renounced(source_code)
    } else {
        // Nếu không có source code, kiểm tra owner_address
        if let Some(ref owner) = contract_info.owner_address {
            // Kiểm tra nếu owner là địa chỉ 0 hoặc dead_address
            owner == "0x0000000000000000000000000000000000000000" || 
            owner.to_lowercase() == "0x000000000000000000000000000000000000dead"
        } else {
            false
        }
    }
}

/// Kiểm tra xem contract có phải là proxy contract không
pub fn is_proxy_contract(contract_info: &ContractInfo) -> bool {
    if let Some(ref source_code) = contract_info.source_code {
        // Kiểm tra các pattern proxy phổ biến
        let proxy_patterns = [
            "delegatecall", 
            "upgradeable", 
            "upgradeablility", 
            "proxy", 
            "implementation()", 
            "_implementation",
            "delegateToImplementation",
            "ERC1967Proxy",
            "TransparentUpgradeableProxy",
            "BeaconProxy",
            "UUPS",
        ];
        
        for pattern in proxy_patterns {
            if source_code.contains(pattern) {
                return true;
            }
        }
        
        // Kiểm tra inheritance từ các contract proxy
        let proxy_inheritance_patterns = [
            "contract \\w+ is \\w*Proxy\\w*",
            "contract \\w+ is \\w*Upgradeable\\w*",
            "import [\"']@openzeppelin/contracts/proxy",
            "import [\"']@openzeppelin/contracts-upgradeable",
        ];
        
        for pattern in proxy_inheritance_patterns {
            if Regex::new(pattern).map_or(false, |re| re.is_match(source_code)) {
                return true;
            }
        }
    }
    
    // Nếu có bytecode, kiểm tra opcode delegatecall
    if let Some(ref bytecode) = contract_info.bytecode {
        if bytecode.contains("delegatecall") || bytecode.contains("DELEGATECALL") {
            return true;
        }
    }
    
    false
}

/// Phân tích các quyền hạn khác của owner (mint, burn, pause)
fn analyze_other_privileges(contract_info: &ContractInfo) -> (bool, bool, bool) {
    let mut has_mint_authority = false;
    let mut has_burn_authority = false;
    let mut has_pause_authority = false;
    
    if let Some(ref source_code) = contract_info.source_code {
        // Kiểm tra quyền mint
        let mint_patterns = [
            "function mint\\(",
            "onlyOwner.+mint\\(",
            "_mint\\(", 
            "mint.*onlyOwner",
        ];
        
        for pattern in mint_patterns {
            if Regex::new(pattern).map_or(false, |re| re.is_match(source_code)) {
                has_mint_authority = true;
                break;
            }
        }
        
        // Kiểm tra quyền burn
        let burn_patterns = [
            "function burn\\(",
            "onlyOwner.+burn\\(",
            "_burn\\(", 
            "burn.*onlyOwner",
        ];
        
        for pattern in burn_patterns {
            if Regex::new(pattern).map_or(false, |re| re.is_match(source_code)) {
                has_burn_authority = true;
                break;
            }
        }
        
        // Kiểm tra quyền pause
        let pause_patterns = [
            "function pause\\(",
            "onlyOwner.+pause\\(",
            "pausable",
            "whenNotPaused",
            "whenPaused",
            "pause.*onlyOwner",
        ];
        
        for pattern in pause_patterns {
            if Regex::new(pattern).map_or(false, |re| re.is_match(source_code)) {
                has_pause_authority = true;
                break;
            }
        }
    }
    
    (has_mint_authority, has_burn_authority, has_pause_authority)
}

/// Analyzer cho quyền owner
struct OwnershipAnalyzer {
    renounce_patterns: Vec<&'static str>,
    backdoor_patterns: Vec<&'static str>,
    multisig_patterns: Vec<&'static str>,
}

impl OwnershipAnalyzer {
    fn new() -> Self {
        Self {
            renounce_patterns: vec![
                "renounceOwnership",
                "transferOwnership\\s*\\(\\s*address\\s*\\(\\s*0\\s*\\)\\s*\\)",
                "_transferOwnership\\s*\\(\\s*address\\s*\\(\\s*0\\s*\\)\\s*\\)",
                "_owner\\s*=\\s*address\\s*\\(\\s*0\\s*\\)",
                "address\\(0\\).*_owner",
                "zero_address.*owner",
                "dead_address.*owner",
            ],
            backdoor_patterns: vec![
                "initializeOwner",
                "recoverOwnership",
                "claimOwnership",
                "resetOwner",
                "transferOwnership.*if\\s*\\(\\s*owner\\s*==\\s*address\\s*\\(\\s*0\\s*\\)\\s*\\)",
                "require\\s*\\(\\s*_owner\\s*==\\s*address\\s*\\(\\s*0\\s*\\)\\s*\\)",
            ],
            multisig_patterns: vec![
                "multisig",
                "multi-sig",
                "MultiSigWallet",
                "confirmations",
                "confirmTransaction",
                "Gnosis",
                "threshold",
                "require\\s*\\(\\s*[0-9]+.*confirmations\\s*\\)",
            ],
        }
    }
    
    fn is_ownership_renounced(&self, source_code: &str) -> bool {
        for pattern in &self.renounce_patterns {
            if Regex::new(pattern).map_or(false, |re| re.is_match(source_code)) {
                return true;
            }
        }
        false
    }
    
    fn has_ownership_backdoor(&self, source_code: &str) -> bool {
        for pattern in &self.backdoor_patterns {
            if Regex::new(pattern).map_or(false, |re| re.is_match(source_code)) {
                return true;
            }
        }
        false
    }
    
    fn has_multisig(&self, source_code: &str) -> bool {
        for pattern in &self.multisig_patterns {
            if Regex::new(pattern).map_or(false, |re| re.is_match(source_code)) {
                return true;
            }
        }
        false
    }
} 