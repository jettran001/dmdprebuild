//! Phân tích blacklist/whitelist và giới hạn giao dịch
//!
//! Module này chứa các hàm phân tích liên quan đến:
//! - Phát hiện blacklist/whitelist trong token
//! - Phát hiện cooling time giữa các giao dịch
//! - Phát hiện giới hạn số lượng giao dịch và số lượng token trong ví

use super::types::ContractInfo;

/// Kiểm tra xem token có chứa chức năng blacklist hoặc whitelist không
pub fn has_blacklist_or_whitelist(contract_info: &ContractInfo) -> bool {
    if let Some(ref source_code) = contract_info.source_code {
        let detector = AccessListDetector::new();
        detector.has_blacklist(source_code) || detector.has_whitelist(source_code)
    } else {
        false
    }
}

/// Kiểm tra xem token có giới hạn thời gian giữa các giao dịch
pub fn has_trading_cooldown(contract_info: &ContractInfo) -> bool {
    if let Some(ref source_code) = contract_info.source_code {
        let detector = CooldownDetector::new();
        detector.has_cooldown(source_code)
    } else {
        false
    }
}

/// Kiểm tra xem token có giới hạn số lượng giao dịch hoặc số lượng token trong ví
pub fn has_max_tx_or_wallet_limit(contract_info: &ContractInfo) -> bool {
    if let Some(ref source_code) = contract_info.source_code {
        let tx_detector = TxLimitDetector::new();
        let whale_detector = AntiWhaleDetector::new();
        
        tx_detector.has_tx_limit(source_code) || whale_detector.has_anti_whale(source_code)
    } else {
        false
    }
}

/// Detector cho blacklist/whitelist
struct AccessListDetector {
    whitelist_patterns: Vec<&'static str>,
    blacklist_patterns: Vec<&'static str>,
}

impl AccessListDetector {
    fn new() -> Self {
        Self {
            whitelist_patterns: vec![
                "whitelist",
                "isWhitelisted",
                "_whitelist",
                "whitelistedAddresses?",
                "addToWhitelist",
                "removeFromWhitelist",
                "excludeFromRestriction",
                "onlyPermitted",
                "allowedToTrade",
                "isPermitted",
                "authorizeAddress",
            ],
            blacklist_patterns: vec![
                "blacklist",
                "isBlacklisted",
                "_blacklist",
                "denylist",
                "blocklist", 
                "blacklistAddress",
                "blacklistedAddresses?",
                "addToBlacklist",
                "removeFromBlacklist",
                "banned",
                "isBanned",
                "banAddress",
                "mapping\\s*\\(\\s*address\\s*=>\\s*bool\\s*\\)\\s*(public\\s*)?\\s*blacklisted",
                "mapping\\s*\\(\\s*address\\s*=>\\s*bool\\s*\\)\\s*(public\\s*)?\\s*banned",
            ],
        }
    }
    
    fn has_whitelist(&self, source_code: &str) -> bool {
        for pattern in &self.whitelist_patterns {
            if source_code.contains(pattern) {
                return true;
            }
        }
        false
    }
    
    fn has_blacklist(&self, source_code: &str) -> bool {
        for pattern in &self.blacklist_patterns {
            if source_code.contains(pattern) {
                return true;
            }
        }
        false
    }
}

/// Detector cho giới hạn giao dịch
struct TxLimitDetector {
    patterns: Vec<&'static str>,
}

impl TxLimitDetector {
    fn new() -> Self {
        Self {
            patterns: vec![
                "maxTxAmount",
                "MAX_TX_AMOUNT",
                "maxTransactionAmount",
                "maxTransactionSize",
                "_maxTx",
                "maxTx",
                "require\\s*\\(\\s*amount\\s*<=\\s*maxTxAmount\\s*\\)",
                "if\\s*\\(\\s*amount\\s*>\\s*maxTxAmount\\s*\\)",
                "maxTokensPerTx",
                "maxTransferAmount",
            ],
        }
    }
    
    fn has_tx_limit(&self, source_code: &str) -> bool {
        for pattern in &self.patterns {
            if source_code.contains(pattern) {
                return true;
            }
        }
        false
    }
}

/// Detector cho anti-whale
struct AntiWhaleDetector {
    patterns: Vec<&'static str>,
}

impl AntiWhaleDetector {
    fn new() -> Self {
        Self {
            patterns: vec![
                "maxWalletAmount",
                "MAX_WALLET_AMOUNT",
                "maxWalletSize",
                "maxBalance",
                "maxHoldingAmount",
                "maxTokensPerWallet",
                "require\\s*\\(\\s*balance.*<=\\s*maxWalletAmount\\s*\\)",
                "antiWhale",
                "maxWallet",
                "if\\s*\\(\\s*balance.*>\\s*maxWalletSize\\s*\\)",
            ],
        }
    }
    
    fn has_anti_whale(&self, source_code: &str) -> bool {
        for pattern in &self.patterns {
            if source_code.contains(pattern) {
                return true;
            }
        }
        false
    }
}

/// Detector cho cooldown time
struct CooldownDetector {
    patterns: Vec<&'static str>,
}

impl CooldownDetector {
    fn new() -> Self {
        Self {
            patterns: vec![
                "cooldown",
                "tradingCooldown",
                "lastTradeTime",
                "lastTransferTime",
                "lastTx",
                "require\\s*\\(\\s*.*now\\s*-\\s*lastTradeTime\\s*.*>=\\s*cooldown\\s*\\)",
                "block\\.timestamp\\s*-\\s*lastTx.*>\\s*\\d+",
                "timeDelay",
                "tradingDelay",
                "transferDelay",
                "antiBotTimer",
                "tradingInterval",
            ],
        }
    }
    
    fn has_cooldown(&self, source_code: &str) -> bool {
        for pattern in &self.patterns {
            if source_code.contains(pattern) {
                return true;
            }
        }
        false
    }
} 