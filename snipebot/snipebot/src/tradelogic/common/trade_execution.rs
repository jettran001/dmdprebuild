pub struct SecurityCheckResult {
    pub is_safe: bool,
    pub risk_level: u8,
    pub safe_to_trade: bool,
    pub risk_score: u8,
}

pub struct TradeParams {
    pub chain_id: u64,
} 