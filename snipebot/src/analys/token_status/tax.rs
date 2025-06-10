//! Phân tích tax, dynamic tax, hidden fee
//!
//! Module này chứa các hàm phân tích liên quan đến:
//! - Phân tích tax mua/bán
//! - Phát hiện tax động (có thể thay đổi)
//! - Phát hiện phí ẩn

use regex::Regex;

use super::types::ContractInfo;

/// Phát hiện tax động hoặc tax cao
pub fn detect_dynamic_tax(contract_info: &ContractInfo) -> bool {
    if let Some(ref source_code) = contract_info.source_code {
        let detector = DynamicFeeDetector::new();
        detector.analyze(source_code)
    } else {
        false
    }
}

/// Phát hiện phí ẩn
pub fn detect_hidden_fees(contract_info: &ContractInfo) -> bool {
    if let Some(ref source_code) = contract_info.source_code {
        // Kiểm tra các pattern phí ẩn
        let hidden_fee_patterns = [
            "fee\\s*=.*variable",
            "tax\\s*=.*variable",
            "setFee\\(",
            "updateFee\\(",
            "changeFee\\(",
            "updateTax\\(",
            "changeTax\\(",
            "excludeFromFee",
            "setFeeExempt",
            "setFeePercent",
            "feePercentages?\\[\\w+\\]",
            "dynamicFee",
            "function\\s+set(Buy|Sell)Tax",
            "_calculateFee.*if\\s*\\(\\s*\\w+\\s*==\\s*\\w+\\s*\\)",
        ];
        
        for pattern in hidden_fee_patterns {
            if Regex::new(pattern).map_or(false, |re| re.is_match(source_code)) {
                return true;
            }
        }
        
        // Kiểm tra nếu _transfer có logic riêng
        let custom_transfer_logic = Regex::new(
            "function\\s+_transfer\\s*\\([^)]*\\)\\s*\\{[^}]*if\\s*\\([^)]*\\)\\s*\\{[^}]*\\}[^}]*\\}"
        ).map_or(false, |re| re.is_match(source_code));
        
        if custom_transfer_logic {
            // Kiểm tra nếu có logic taxFee thay đổi trong _transfer
            let dynamic_tax_in_transfer = Regex::new(
                "(taxFee|fee|tax)\\s*=\\s*(\\w+)\\s*;"
            ).map_or(false, |re| re.is_match(source_code));
            
            if dynamic_tax_in_transfer {
                return true;
            }
        }
        
        // Kiểm tra multi-tier fees
        let multi_tier_fees = Regex::new(
            "if\\s*\\(\\s*\\w+\\s*<\\s*\\d+\\s*\\)\\s*\\{[^}]*fee\\s*=\\s*\\d+[^}]*\\}\\s*else\\s*\\{"
        ).map_or(false, |re| re.is_match(source_code));
        
        if multi_tier_fees {
            return true;
        }
    }
    
    false
}

/// Phân tích tỷ lệ tax mua/bán (%)
pub fn analyze_tax_rates(contract_info: &ContractInfo) -> (f64, f64) {
    let mut buy_tax = 0.0;
    let mut sell_tax = 0.0;
    
    if let Some(ref source_code) = contract_info.source_code {
        // Tìm buy tax
        let buy_tax_patterns = [
            "buyFee\\s*=\\s*(\\d+)",
            "buyTax\\s*=\\s*(\\d+)",
            "(buy|Buy)Fee\\s*=\\s*(\\d+)",
            "(buy|Buy)Tax\\s*=\\s*(\\d+)",
            "tax\\.(buy|Buy)\\s*=\\s*(\\d+)",
            "fee\\.(buy|Buy)\\s*=\\s*(\\d+)",
        ];
        
        for pattern in buy_tax_patterns {
            if let Some(captures) = Regex::new(pattern).ok().and_then(|re| re.captures(source_code)) {
                if let Some(tax_str) = captures.get(1).map(|m| m.as_str()) {
                    if let Ok(tax) = tax_str.parse::<f64>() {
                        // Nếu tax > 100, giả sử nó là basis points (1/100 của %)
                        buy_tax = if tax > 100.0 { tax / 100.0 } else { tax };
                        break;
                    }
                }
            }
        }
        
        // Tìm sell tax
        let sell_tax_patterns = [
            "sellFee\\s*=\\s*(\\d+)",
            "sellTax\\s*=\\s*(\\d+)",
            "(sell|Sell)Fee\\s*=\\s*(\\d+)",
            "(sell|Sell)Tax\\s*=\\s*(\\d+)",
            "tax\\.(sell|Sell)\\s*=\\s*(\\d+)",
            "fee\\.(sell|Sell)\\s*=\\s*(\\d+)",
        ];
        
        for pattern in sell_tax_patterns {
            if let Some(captures) = Regex::new(pattern).ok().and_then(|re| re.captures(source_code)) {
                if let Some(tax_str) = captures.get(1).map(|m| m.as_str()) {
                    if let Ok(tax) = tax_str.parse::<f64>() {
                        // Nếu tax > 100, giả sử nó là basis points (1/100 của %)
                        sell_tax = if tax > 100.0 { tax / 100.0 } else { tax };
                        break;
                    }
                }
            }
        }
        
        // Nếu không tìm thấy buy/sell tax riêng, tìm tax chung
        if buy_tax == 0.0 && sell_tax == 0.0 {
            let general_tax_patterns = [
                "fee\\s*=\\s*(\\d+)",
                "tax\\s*=\\s*(\\d+)",
                "taxFee\\s*=\\s*(\\d+)",
                "feePercent\\s*=\\s*(\\d+)",
                "transferFee\\s*=\\s*(\\d+)",
            ];
            
            for pattern in general_tax_patterns {
                if let Some(captures) = Regex::new(pattern).ok().and_then(|re| re.captures(source_code)) {
                    if let Some(tax_str) = captures.get(1).map(|m| m.as_str()) {
                        if let Ok(tax) = tax_str.parse::<f64>() {
                            // Nếu tax > 100, giả sử nó là basis points (1/100 của %)
                            let normalized_tax = if tax > 100.0 { tax / 100.0 } else { tax };
                            buy_tax = normalized_tax;
                            sell_tax = normalized_tax;
                            break;
                        }
                    }
                }
            }
        }
    }
    
    (buy_tax, sell_tax)
}

/// Detector cho tax/fee động
struct DynamicFeeDetector {
    patterns: Vec<&'static str>,
}

impl DynamicFeeDetector {
    fn new() -> Self {
        Self {
            patterns: vec![
                // Set fee functions
                "function\\s+setFee",
                "function\\s+updateFee",
                "function\\s+changeFee",
                "function\\s+setBuyFee",
                "function\\s+setSellFee",
                "function\\s+setTax",
                "function\\s+updateTax",
                "function\\s+changeTax",
                "function\\s+setBuyTax",
                "function\\s+setSellTax",
                
                // Dynamic fee variables
                "uint256\\s+public\\s+\\w+Fee\\s*=\\s*\\d+.*changeable",
                "uint256\\s+public\\s+\\w+Tax\\s*=\\s*\\d+.*changeable",
                
                // Methods to exclude from fee
                "function\\s+excludeFromFee",
                "function\\s+includeInFee",
                "function\\s+setFeeExempt",
                
                // Fee modifiers
                "tax.*isExcluded",
                "fee.*isExcluded",
            ],
        }
    }
    
    fn analyze(&self, source_code: &str) -> bool {
        for pattern in &self.patterns {
            if Regex::new(pattern).map_or(false, |re| re.is_match(source_code)) {
                return true;
            }
        }
        false
    }
} 