//! Security Module cho Smart Trade System
//! 
//! Module này chịu trách nhiệm cho việc kiểm tra bảo mật, phát hiện gian lận
//! và các biện pháp bảo vệ người dùng trong quá trình giao dịch.

// External imports
use std::sync::Arc;
use std::collections::HashMap;
use anyhow::Result;
use tracing::{debug, info, warn};

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;

use super::token_analysis::analyze_token;

/// Cấu hình bảo mật cho trades
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Có tự động bán khi phát hiện rủi ro cao
    pub auto_sell_on_high_risk: bool,
    
    /// Mức rủi ro tối đa chấp nhận được (0-100)
    pub max_acceptable_risk: u8,
    
    /// Các loại rủi ro cần kiểm tra
    pub risk_checks_enabled: HashMap<String, bool>,
    
    /// Thời gian giữa các lần kiểm tra (seconds)
    pub check_interval_seconds: u64,
    
    /// Có kiểm tra thay đổi trong owner privileges
    pub check_owner_changes: bool,
    
    /// Có kiểm tra tax động
    pub check_dynamic_tax: bool,
    
    /// Có kiểm tra thay đổi liquidity
    pub check_liquidity_changes: bool,
    
    /// Có kiểm tra honeypot
    pub check_honeypot: bool,
    
    /// Có kiểm tra blacklist
    pub check_blacklist: bool,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        let mut risk_checks = HashMap::new();
        risk_checks.insert("honeypot".to_string(), true);
        risk_checks.insert("dynamic_tax".to_string(), true);
        risk_checks.insert("owner_privileges".to_string(), true);
        risk_checks.insert("liquidity_removal".to_string(), true);
        risk_checks.insert("blacklist".to_string(), true);
        
        Self {
            auto_sell_on_high_risk: true,
            max_acceptable_risk: 70,
            risk_checks_enabled: risk_checks,
            check_interval_seconds: 60,
            check_owner_changes: true,
            check_dynamic_tax: true,
            check_liquidity_changes: true,
            check_honeypot: true,
            check_blacklist: true,
        }
    }
}

/// Chi tiết về kiểm tra an toàn token
#[derive(Debug, Clone)]
pub struct TokenSecurityCheck {
    /// Token address
    pub token_address: String,
    
    /// Chain ID
    pub chain_id: u32,
    
    /// Danh sách các vấn đề phát hiện được
    pub issues: Vec<TokenIssue>,
    
    /// Có an toàn không
    pub is_safe: bool,
    
    /// Mức độ rủi ro (0-100)
    pub risk_score: u8,
    
    /// Chi tiết các vấn đề
    pub details: HashMap<String, String>,
    
    /// Timestamp kiểm tra
    pub timestamp: u64,
}

/// Kết quả kiểm tra an toàn
#[derive(Debug, Clone)]
pub struct SecurityCheckResult {
    /// Có an toàn không
    pub is_safe: bool,
    
    /// Danh sách các vấn đề
    pub issues: Vec<String>,
    
    /// Mô tả chi tiết
    pub description: Option<String>,
    
    /// Có nên bán ngay không
    pub should_sell_immediately: bool,
    
    /// Điểm rủi ro (0-100)
    pub risk_score: u8,
}

/// Quản lý bảo mật cho SmartTradeExecutor
pub struct SecurityManager {
    /// Cấu hình bảo mật
    pub config: SecurityConfig,
    
    /// Cache kết quả kiểm tra
    cache: HashMap<String, TokenSecurityCheck>,
    
    /// Danh sách các token được coi là an toàn
    whitelisted_tokens: HashMap<u32, Vec<String>>,
    
    /// Danh sách các token được coi là không an toàn
    blacklisted_tokens: HashMap<u32, Vec<String>>,
}

impl SecurityManager {
    /// Tạo SecurityManager mới với cấu hình mặc định
    pub fn new() -> Self {
        Self {
            config: SecurityConfig::default(),
            cache: HashMap::new(),
            whitelisted_tokens: HashMap::new(),
            blacklisted_tokens: HashMap::new(),
        }
    }
    
    /// Tạo SecurityManager với cấu hình tùy chỉnh
    pub fn with_config(config: SecurityConfig) -> Self {
        Self {
            config,
            cache: HashMap::new(),
            whitelisted_tokens: HashMap::new(),
            blacklisted_tokens: HashMap::new(),
        }
    }
    
    /// Thêm token vào whitelist
    pub fn add_to_whitelist(&mut self, chain_id: u32, token_address: &str) {
        let token_list = self.whitelisted_tokens.entry(chain_id).or_insert_with(Vec::new);
        if !token_list.contains(&token_address.to_string()) {
            token_list.push(token_address.to_string());
            debug!("Đã thêm token {} vào whitelist cho chain {}", token_address, chain_id);
        }
    }
    
    /// Thêm token vào blacklist
    pub fn add_to_blacklist(&mut self, chain_id: u32, token_address: &str) {
        let token_list = self.blacklisted_tokens.entry(chain_id).or_insert_with(Vec::new);
        if !token_list.contains(&token_address.to_string()) {
            token_list.push(token_address.to_string());
            warn!("Đã thêm token {} vào blacklist cho chain {}", token_address, chain_id);
        }
    }
    
    /// Kiểm tra token có an toàn không
    /// 
    /// # Tham số
    /// * `chain_id` - ID của blockchain
    /// * `token_address` - Địa chỉ token
    /// * `adapter` - EVM adapter để tương tác với blockchain
    ///
    /// # Returns
    /// * `Result<SecurityCheckResult>` - Kết quả kiểm tra
    pub async fn check_token_security(
        &mut self,
        chain_id: u32,
        token_address: &str,
        adapter: &Arc<EvmAdapter>
    ) -> Result<SecurityCheckResult> {
        // Kiểm tra whitelist/blacklist
        if let Some(tokens) = self.whitelisted_tokens.get(&chain_id) {
            if tokens.contains(&token_address.to_string()) {
                return Ok(SecurityCheckResult {
                    is_safe: true,
                    issues: vec![],
                    description: Some("Token trong whitelist".to_string()),
                    should_sell_immediately: false,
                    risk_score: 0,
                });
            }
        }
        
        if let Some(tokens) = self.blacklisted_tokens.get(&chain_id) {
            if tokens.contains(&token_address.to_string()) {
                return Ok(SecurityCheckResult {
                    is_safe: false,
                    issues: vec!["Token trong blacklist".to_string()],
                    description: Some("Token đã được đánh dấu không an toàn".to_string()),
                    should_sell_immediately: true,
                    risk_score: 100,
                });
            }
        }
        
        // Kiểm tra cache
        let cache_key = format!("{}:{}", chain_id, token_address);
        let now = chrono::Utc::now().timestamp() as u64;
        
        if let Some(cached) = self.cache.get(&cache_key) {
            // Sử dụng cache nếu chưa quá 10 phút
            if now - cached.timestamp < 600 {
                debug!("Sử dụng kết quả cache cho token {}", token_address);
                return Ok(SecurityCheckResult {
                    is_safe: cached.is_safe,
                    issues: cached.issues.iter().map(|i| format!("{:?}", i)).collect(),
                    description: Some(format!("Từ cache: {} vấn đề phát hiện được", cached.issues.len())),
                    should_sell_immediately: !cached.is_safe && cached.risk_score > self.config.max_acceptable_risk,
                    risk_score: cached.risk_score,
                });
            }
        }
        
        // Thực hiện kiểm tra
        debug!("Kiểm tra bảo mật cho token {} trên chain {}", token_address, chain_id);
        
        // 1. Kiểm tra honeypot nếu được bật
        let mut issues = Vec::new();
        let mut details = HashMap::new();
        let mut risk_score = 0;
        
        if self.config.check_honeypot {
            match self.check_honeypot(chain_id, token_address, adapter).await {
                Ok((is_honeypot, reason)) => {
                    if is_honeypot {
                        issues.push(TokenIssue::Honeypot);
                        risk_score += 40;
                        if let Some(reason_str) = reason {
                            details.insert("honeypot_reason".to_string(), reason_str);
                        }
                    }
                },
                Err(e) => {
                    warn!("Lỗi khi kiểm tra honeypot: {}", e);
                    details.insert("honeypot_error".to_string(), format!("{}", e));
                }
            }
        }
        
        // 2. Kiểm tra dynamic tax nếu được bật
        if self.config.check_dynamic_tax {
            match self.check_dynamic_tax(chain_id, token_address, adapter).await {
                Ok((has_dynamic_tax, reason)) => {
                    if has_dynamic_tax {
                        issues.push(TokenIssue::DynamicTax);
                        risk_score += 30;
                        if let Some(reason_str) = reason {
                            details.insert("dynamic_tax_reason".to_string(), reason_str);
                        }
                    }
                },
                Err(e) => {
                    warn!("Lỗi khi kiểm tra dynamic tax: {}", e);
                    details.insert("dynamic_tax_error".to_string(), format!("{}", e));
                }
            }
        }
        
        // 3. Kiểm tra owner privileges nếu được bật
        if self.config.check_owner_changes {
            match self.check_owner_privileges(chain_id, token_address, adapter).await {
                Ok((has_privileges, privileges)) => {
                    if has_privileges && !privileges.is_empty() {
                        issues.push(TokenIssue::OwnerPrivilege);
                        risk_score += 20;
                        details.insert("owner_privileges".to_string(), privileges.join(", "));
                    }
                },
                Err(e) => {
                    warn!("Lỗi khi kiểm tra owner privileges: {}", e);
                    details.insert("owner_privileges_error".to_string(), format!("{}", e));
                }
            }
        }
        
        // 4. Kiểm tra liquidity changes nếu được bật
        if self.config.check_liquidity_changes {
            match self.check_liquidity_risk(chain_id, token_address, adapter).await {
                Ok((has_liquidity_risk, reason)) => {
                    if has_liquidity_risk {
                        issues.push(TokenIssue::LiquidityRisk);
                        risk_score += 25;
                        if let Some(reason_str) = reason {
                            details.insert("liquidity_risk_reason".to_string(), reason_str);
                        }
                    }
                },
                Err(e) => {
                    warn!("Lỗi khi kiểm tra liquidity risk: {}", e);
                    details.insert("liquidity_risk_error".to_string(), format!("{}", e));
                }
            }
        }
        
        // 5. Kiểm tra blacklist nếu được bật
        if self.config.check_blacklist {
            match self.check_blacklist(chain_id, token_address, adapter).await {
                Ok((has_blacklist, blacklist)) => {
                    if has_blacklist {
                        issues.push(TokenIssue::Blacklist);
                        risk_score += 15;
                        if let Some(details_vec) = blacklist {
                            details.insert("blacklist_details".to_string(), details_vec.join(", "));
                        }
                    }
                },
                Err(e) => {
                    warn!("Lỗi khi kiểm tra blacklist: {}", e);
                    details.insert("blacklist_error".to_string(), format!("{}", e));
                }
            }
        }
        
        // Đảm bảo risk score nằm trong khoảng 0-100
        risk_score = risk_score.min(100);
        
        // Tạo kết quả và lưu vào cache
        let is_safe = risk_score <= self.config.max_acceptable_risk;
        let should_sell = !is_safe && self.config.auto_sell_on_high_risk;
        
        let security_check = TokenSecurityCheck {
            token_address: token_address.to_string(),
            chain_id,
            issues: issues.clone(),
            is_safe,
            risk_score,
            details: details.clone(),
            timestamp: now,
        };
        
        // Cập nhật cache
        self.cache.insert(cache_key, security_check);
        
        // Tự động cập nhật blacklist nếu risk_score = 100
        if risk_score >= 100 {
            self.add_to_blacklist(chain_id, token_address);
        }
        
        let result = SecurityCheckResult {
            is_safe,
            issues: issues.iter().map(|i| format!("{:?}", i)).collect(),
            description: Some(format!("Risk score: {}/100, {} vấn đề phát hiện được", 
                risk_score, issues.len())),
            should_sell_immediately: should_sell,
            risk_score,
        };
        
        debug!("Kết quả kiểm tra token {}: safe={}, risk_score={}, issues={}",
              token_address, is_safe, risk_score, issues.len());
        
        Ok(result)
    }
    
    /// Kiểm tra honeypot
    async fn check_honeypot(
        &self,
        chain_id: u32,
        token_address: &str,
        adapter: &Arc<EvmAdapter>
    ) -> Result<(bool, Option<String>)> {
        // Mô phỏng giao dịch bán để phát hiện honeypot
        let small_amount = 0.01; // 0.01 ETH/BNB worth
        let large_amount = 0.5;  // 0.5 ETH/BNB worth
        
        // Mô phỏng bán lượng nhỏ
        let small_simulation = adapter.simulate_token_sell(token_address, small_amount)
            .await.context("Không thể mô phỏng bán lượng nhỏ")?;
        
        if !small_simulation.can_sell {
            return Ok((true, Some(format!("Không thể bán: {}", 
                small_simulation.error.unwrap_or_else(|| "Unknown error".to_string())))));
        }
        
        // Mô phỏng bán lượng lớn
        let large_simulation = adapter.simulate_token_sell(token_address, large_amount)
            .await.context("Không thể mô phỏng bán lượng lớn")?;
        
        if !large_simulation.can_sell {
            return Ok((true, Some(format!("Không thể bán lượng lớn: {}", 
                large_simulation.error.unwrap_or_else(|| "Unknown error".to_string())))));
        }
        
        // Kiểm tra slippage quá cao
        if let (Some(small_price), Some(large_price)) = (small_simulation.price_impact, large_simulation.price_impact) {
            if large_price > 50.0 {
                return Ok((true, Some(format!("Slippage quá cao khi bán lượng lớn: {:.1}%", large_price))));
            }
            
            if large_price > 3.0 * small_price {
                return Ok((true, Some(format!(
                    "Slippage không tuyến tính: {:.1}% (nhỏ) vs {:.1}% (lớn)", 
                    small_price, large_price
                ))));
            }
        }
        
        // Kiểm tra các lỗi khác
        if let Some(error) = &large_simulation.error {
            if error.contains("cannot sell") || 
               error.contains("transfer failed") || 
               error.contains("insufficient") {
                return Ok((true, Some(format!("Lỗi bán: {}", error))));
            }
        }
        
        // Kiểm tra buy/sell tax
        if small_simulation.sell_tax.unwrap_or(0.0) > 30.0 {
            return Ok((true, Some(format!(
                "Sell tax quá cao: {:.1}%", 
                small_simulation.sell_tax.unwrap_or(0.0)
            ))));
        }
        
        Ok((false, None))
    }
    
    /// Kiểm tra dynamic tax
    async fn check_dynamic_tax(
        &self,
        chain_id: u32,
        token_address: &str,
        adapter: &Arc<EvmAdapter>
    ) -> Result<(bool, Option<String>)> {
        // Mô phỏng nhiều giao dịch mua/bán để phát hiện tax động
        let amount = 0.1; // 0.1 ETH/BNB worth
        
        // Mô phỏng lần 1
        let sim1 = adapter.simulate_buy_sell_slippage(token_address, amount)
            .await.context("Không thể mô phỏng mua/bán lần 1")?;
        
        // Delay một chút
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        
        // Mô phỏng lần 2
        let sim2 = adapter.simulate_buy_sell_slippage(token_address, amount)
            .await.context("Không thể mô phỏng mua/bán lần 2")?;
        
        // Delay một chút
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        
        // Mô phỏng lần 3
        let sim3 = adapter.simulate_buy_sell_slippage(token_address, amount)
            .await.context("Không thể mô phỏng mua/bán lần 3")?;
        
        // So sánh tax giữa các lần
        let buy_tax1 = sim1.buy_tax.unwrap_or(0.0);
        let buy_tax2 = sim2.buy_tax.unwrap_or(0.0);
        let buy_tax3 = sim3.buy_tax.unwrap_or(0.0);
        
        let sell_tax1 = sim1.sell_tax.unwrap_or(0.0);
        let sell_tax2 = sim2.sell_tax.unwrap_or(0.0);
        let sell_tax3 = sim3.sell_tax.unwrap_or(0.0);
        
        // Phát hiện tax động
        let buy_tax_diff = f64::max(f64::max((buy_tax1 - buy_tax2).abs(), (buy_tax2 - buy_tax3).abs()), (buy_tax1 - buy_tax3).abs());
        let sell_tax_diff = f64::max(f64::max((sell_tax1 - sell_tax2).abs(), (sell_tax2 - sell_tax3).abs()), (sell_tax1 - sell_tax3).abs());
        
        if buy_tax_diff > 3.0 || sell_tax_diff > 3.0 {
            return Ok((true, Some(format!(
                "Phát hiện tax động: Buy tax {:.1}% -> {:.1}% -> {:.1}%, Sell tax {:.1}% -> {:.1}% -> {:.1}%",
                buy_tax1, buy_tax2, buy_tax3, sell_tax1, sell_tax2, sell_tax3
            ))));
        }
        
        // Kiểm tra tax quá cao
        if buy_tax3 > 20.0 || sell_tax3 > 20.0 {
            return Ok((true, Some(format!(
                "Tax quá cao: Buy tax {:.1}%, Sell tax {:.1}%",
                buy_tax3, sell_tax3
            ))));
        }
        
        Ok((false, None))
    }
    
    /// Kiểm tra owner privileges
    async fn check_owner_privileges(
        &self,
        chain_id: u32,
        token_address: &str,
        adapter: &Arc<EvmAdapter>
    ) -> Result<(bool, Vec<String>)> {
        // Lấy thông tin contract để phát hiện các owner privilege
        let contract_info = adapter.get_contract_info(token_address)
            .await.context("Không thể lấy thông tin contract")?;
        
        let mut privileges = Vec::new();
        
        // Phát hiện các quyền đặc biệt
        if contract_info.can_mint {
            privileges.push("Can mint arbitrary tokens".to_string());
        }
        
        if contract_info.can_blacklist {
            privileges.push("Can blacklist addresses".to_string());
        }
        
        if contract_info.can_change_tax {
            privileges.push("Can change tax".to_string());
        }
        
        if contract_info.can_pause_trading {
            privileges.push("Can pause trading".to_string());
        }
        
        if contract_info.is_proxy {
            privileges.push("Is proxy/upgradeable contract".to_string());
        }
        
        if contract_info.can_whitelist {
            privileges.push("Can whitelist addresses".to_string());
        }
        
        // Phát hiện quyền rút liquidity
        if contract_info.can_take_back_ownership {
            privileges.push("Can take back ownership/recover assets".to_string());
        }
        
        if contract_info.has_hidden_owner {
            privileges.push("Has hidden owner variable".to_string());
        }
        
        // Kiểm tra nếu owner có thể rút token/liquidity
        if contract_info.can_withdraw_token || contract_info.can_withdraw_eth {
            privileges.push("Can withdraw tokens/ETH".to_string());
        }
        
        Ok((!privileges.is_empty(), privileges))
    }
    
    /// Kiểm tra liquidity risk
    async fn check_liquidity_risk(
        &self,
        chain_id: u32,
        token_address: &str,
        adapter: &Arc<EvmAdapter>
    ) -> Result<(bool, Option<String>)> {
        // Lấy thông tin liquidity
        let liquidity_info = adapter.get_token_liquidity(token_address)
            .await.context("Không thể lấy thông tin thanh khoản")?;
        
        // Kiểm tra thanh khoản quá thấp
        if liquidity_info.liquidity_usd < 10000.0 {
            return Ok((true, Some(format!(
                "Thanh khoản quá thấp: ${:.2}", liquidity_info.liquidity_usd
            ))));
        }
        
        // Kiểm tra liquidity lock
        let lock_status = adapter.check_liquidity_locked(token_address)
            .await.context("Không thể kiểm tra khóa thanh khoản")?;
        
        if !lock_status.is_locked {
            return Ok((true, Some("Thanh khoản không được khóa".to_string())));
        } else if lock_status.lock_time_seconds < 7 * 24 * 3600 {
            return Ok((true, Some(format!(
                "Thời gian khóa quá ngắn: {} ngày", 
                lock_status.lock_time_seconds / (24 * 3600)
            ))));
        }
        
        // Kiểm tra % của liquidity được khóa
        if lock_status.locked_percent < 80.0 {
            return Ok((true, Some(format!(
                "Chỉ {:.1}% thanh khoản được khóa, cần ít nhất 80%",
                lock_status.locked_percent
            ))));
        }
        
        // Kiểm tra lịch sử biến động thanh khoản
        let recent_events = adapter.get_recent_liquidity_events(token_address, 30)
            .await.context("Không thể lấy lịch sử liquidity")?;
        
        let mut suspicious_events = 0;
        for event in &recent_events {
            if event.event_type == "remove" && event.amount_usd > 5000.0 {
                suspicious_events += 1;
            }
        }
        
        if suspicious_events > 2 {
            return Ok((true, Some(format!(
                "Phát hiện {} lần rút thanh khoản lớn trong 30 ngày gần đây",
                suspicious_events
            ))));
        }
        
        Ok((false, None))
    }
    
    /// Kiểm tra blacklist
    async fn check_blacklist(
        &self,
        chain_id: u32,
        token_address: &str,
        adapter: &Arc<EvmAdapter>
    ) -> Result<(bool, Option<Vec<String>>)> {
        // Lấy thông tin contract
        let contract_info = adapter.get_contract_info(token_address)
            .await.context("Không thể lấy thông tin contract")?;
        
        let mut blacklist_features = Vec::new();
        
        // Phát hiện blacklist/whitelist
        if contract_info.can_blacklist || contract_info.has_blacklist {
            blacklist_features.push("Has blacklist function or mapping".to_string());
        }
        
        if contract_info.can_whitelist || contract_info.has_whitelist {
            blacklist_features.push("Has whitelist function or mapping".to_string());
        }
        
        // Phát hiện max tx limit
        if contract_info.has_max_tx_limit {
            blacklist_features.push(format!(
                "Has max transaction limit: {} tokens", 
                contract_info.max_tx_amount.unwrap_or(0.0)
            ));
        }
        
        // Phát hiện max wallet limit
        if contract_info.has_max_wallet_limit {
            blacklist_features.push(format!(
                "Has max wallet limit: {} tokens",
                contract_info.max_wallet_amount.unwrap_or(0.0)
            ));
        }
        
        // Phát hiện trading cooldown
        if contract_info.has_trading_cooldown {
            blacklist_features.push("Has trading cooldown".to_string());
        }
        
        // Phát hiện anti-bot measures
        if contract_info.has_anti_bot {
            blacklist_features.push("Has anti-bot measures".to_string());
        }
        
        Ok((!blacklist_features.is_empty(), 
            if blacklist_features.is_empty() { None } else { Some(blacklist_features) }
        ))
    }
}

impl SmartTradeExecutor {
    /// Lấy thông tin chi tiết contract từ blockchain
    ///
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    /// - `adapter`: EVM adapter
    ///
    /// # Returns
    /// - `Option<ContractInfo>`: Thông tin contract nếu có
    pub async fn get_contract_info(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> Option<ContractInfo> {
        match adapter.get_contract_info(token_address).await {
            Ok(contract_info) => Some(contract_info),
            Err(e) => {
                warn!("Không thể lấy thông tin contract {}: {:?}", token_address, e);
                None
            }
        }
    }
    
    /// Phân tích rủi ro token dựa trên đặc điểm contract và hành vi thị trường
    ///
    /// Sử dụng RiskAnalyzer từ analys/risk_analyzer để thực hiện phân tích toàn diện
    ///
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    /// - `adapter`: EVM adapter
    ///
    /// # Returns
    /// - `anyhow::Result<(f64, Vec<RiskFactor>, TradeRecommendation)>`: Kết quả phân tích an toàn
    pub async fn analyze_security_risk(
        &self, 
        chain_id: u32, 
        token_address: &str, 
        adapter: &Arc<EvmAdapter>
    ) -> anyhow::Result<(f64, Vec<RiskFactor>, TradeRecommendation)> {
        // Lấy thông tin contract
        let contract_info = match self.get_contract_info(chain_id, token_address, adapter).await {
            Some(info) => info,
            None => {
                warn!("Không thể lấy thông tin contract cho {}", token_address);
                return Ok((90.0, vec![RiskFactor::ContractUnavailable(90.0)], TradeRecommendation::Avoid));
            }
        };
        
        // Sử dụng RiskAnalyzer từ module analys
        let risk_analyzer = self.risk_analyzer.read().await;
        
        // Thu thập thêm dữ liệu cần thiết
        let liquidity_events = adapter.get_liquidity_events(token_address, 10).await.ok();
        let price_history = adapter.get_token_price_history(token_address, 24).await.ok();
        let tax_info = adapter.get_token_tax_info(token_address).await.ok();
        let volume_data = adapter.get_token_volume_history(token_address, 7).await.ok();
        
        // Phân tích rủi ro sử dụng RiskAnalyzer
        let analysis = risk_analyzer.analyze_trade_risk(
            token_address,
            chain_id,
            Some(&contract_info),
            liquidity_events.as_deref(),
            price_history.as_deref(),
            tax_info.as_ref(),
            volume_data.as_deref()
        );
        
        // Chuyển đổi sang kết quả mong muốn
        let risk_factors: Vec<RiskFactor> = analysis.risk_factors
            .into_iter()
            .map(|(factor, score)| factor)
            .collect();
        
        // Log kết quả phân tích
        info!(
            "Phân tích rủi ro token {}: Điểm={:.1}, Khuyến nghị={:?}, Yếu tố={:?}",
            token_address, analysis.risk_score, analysis.trade_recommendation, risk_factors
        );
        
        Ok((analysis.risk_score, risk_factors, analysis.trade_recommendation))
    }
    
    /// Kiểm tra xem token có thể giao dịch (trade) hay không
    ///
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    /// - `adapter`: EVM adapter
    ///
    /// # Returns
    /// - `(bool, String)`: (Là honeypot?, lý do)
    pub async fn check_token_tradability(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> (bool, String) {
        let mut reasons = Vec::new();
        
        // 1. Thử mô phỏng giao dịch mua và bán
        let is_honeypot = self.detect_honeypot(chain_id, token_address, adapter).await;
        if is_honeypot {
            reasons.push("Token không thể bán (honeypot)".to_string());
        }
        
        // 2. Kiểm tra tax
        let (has_tax_issues, buy_tax, sell_tax) = self.detect_dynamic_tax(chain_id, token_address, adapter).await;
        if has_tax_issues {
            if sell_tax > MAX_SAFE_SELL_TAX {
                reasons.push(format!("Sell tax quá cao: {}%", sell_tax));
            }
            if buy_tax > MAX_SAFE_BUY_TAX {
                reasons.push(format!("Buy tax quá cao: {}%", buy_tax));
            }
        }
        
        // 3. Kiểm tra thanh khoản
        let (liquidity_risk, liquidity_reason) = self.detect_liquidity_risk(chain_id, token_address, adapter).await;
        if liquidity_risk {
            reasons.push(format!("Rủi ro thanh khoản: {}", liquidity_reason));
        }
        
        // 4. Kiểm tra giới hạn giao dịch
        let (has_tx_limit, _, limit_percent) = self.detect_transaction_limits(chain_id, token_address, adapter).await;
        if has_tx_limit && limit_percent < 0.5 {
            // Nếu giới hạn giao dịch quá thấp (<0.5% của tổng supply)
            reasons.push(format!("Giới hạn giao dịch quá thấp: {}%", limit_percent));
        }
        
        // Kết luận
        let is_tradable = reasons.is_empty();
        let reason = if is_tradable {
            "Token có thể giao dịch bình thường".to_string()
        } else {
            reasons.join("; ")
        };
        
        (!is_tradable, reason)
    }
    
    /// Kiểm tra token có trong danh sách an toàn không
    ///
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    ///
    /// # Returns
    /// - `bool`: true nếu token an toàn
    pub async fn is_in_safe_list(&self, chain_id: u32, token_address: &str) -> bool {
        if let Some(safe_tokens) = self.safe_token_lists.get(&chain_id) {
            let token_key = token_address.to_lowercase();
            safe_tokens.contains(&token_key)
        } else {
            false
        }
    }
    
    /// Kiểm tra token có trong danh sách đen không
    ///
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    ///
    /// # Returns
    /// - `bool`: true nếu token trong danh sách đen
    pub async fn is_in_blacklist(&self, chain_id: u32, token_address: &str) -> bool {
        if let Some(blacklist) = self.blacklisted_tokens.get(&chain_id) {
            let token_key = token_address.to_lowercase();
            blacklist.contains(&token_key)
        } else {
            false
        }
    }
    
    /// Kiểm tra xem hợp đồng có được xác minh không (verified)
    ///
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    /// - `adapter`: EVM adapter
    ///
    /// # Returns
    /// - `bool`: true nếu hợp đồng đã được xác minh
    pub async fn is_contract_verified(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> bool {
        if let Some(contract_info) = self.get_contract_info(chain_id, token_address, adapter).await {
            contract_info.is_verified && contract_info.source_code.is_some()
        } else {
            false
        }
    }
    
    /// Cảnh báo người dùng các rủi ro tiềm ẩn
    ///
    /// # Parameters
    /// - `token_address`: Địa chỉ token
    /// - `risk_score`: Điểm rủi ro
    /// - `risk_factors`: Danh sách yếu tố rủi ro
    /// - `chain_id`: ID của blockchain
    pub async fn warn_user_of_risks(&self, token_address: &str, risk_score: f64, risk_factors: &[RiskFactor], chain_id: u32) {
        // Xác định mức độ nghiêm trọng (0-5)
        let severity = if risk_score >= 80.0 {
            5 // Rất nguy hiểm
        } else if risk_score >= 60.0 {
            4 // Nguy hiểm
        } else if risk_score >= 40.0 {
            3 // Rủi ro đáng kể
        } else if risk_score >= 20.0 {
            2 // Cần thận trọng
        } else {
            1 // Rủi ro thấp
        };
        
        // Tạo thông điệp cảnh báo
        let mut warning_message = format!("RỦI RO TOKEN - Điểm: {:.1}/100 - ", risk_score);
        
        match severity {
            5 => warning_message.push_str("NGUY HIỂM NGHIÊM TRỌNG"),
            4 => warning_message.push_str("NGUY HIỂM CAO"),
            3 => warning_message.push_str("RỦI RO ĐÁNG KỂ"),
            2 => warning_message.push_str("CẦN THẬN TRỌNG"),
            _ => warning_message.push_str("RỦI RO THẤP"),
        }
        
        if !risk_factors.is_empty() {
            warning_message.push_str("\nYếu tố rủi ro: ");
            for factor in risk_factors {
                let factor_str = match factor {
                    RiskFactor::OwnerPrivileges(_) => "Quyền owner không bị từ bỏ",
                    RiskFactor::Honeypot(_) => "Dấu hiệu honeypot",
                    RiskFactor::HighTax(_) => "Tax cao bất thường",
                    RiskFactor::LowLiquidity(_) => "Thanh khoản thấp",
                    RiskFactor::ContractUnavailable(_) => "Không thể phân tích contract",
                    RiskFactor::BytecodeIssues(_) => "Vấn đề với bytecode",
                    RiskFactor::ContractStructure(_) => "Cấu trúc contract đáng ngờ",
                    _ => "Rủi ro khác",
                };
                warning_message.push_str(&format!("\n- {}", factor_str));
            }
        }
        
        // Gửi cảnh báo đến người dùng
        self.send_alert(
            token_address,
            "token_risk",
            &warning_message,
            severity,
            chain_id
        ).await;
    }
    
    /// Dùng trí tuệ nhóm để phân tích token (từ API bên ngoài)
    ///
    /// # Parameters
    /// - `token_address`: Địa chỉ token
    /// - `chain_id`: ID của blockchain
    ///
    /// # Returns
    /// - `(bool, Option<f64>, HashMap<String, String>)`: (Có rủi ro, điểm an toàn, dữ liệu)
    pub async fn crowd_sourced_analysis(&self, token_address: &str, chain_id: u32) -> (bool, Option<f64>, HashMap<String, String>) {
        let mut all_data = HashMap::new();
        let mut is_risky = false;
        let mut safety_score: Option<f64> = None;
        
        // Lấy dữ liệu từ các API bên ngoài
        if let Some(data) = self.fetch_external_token_data(token_address, &format!("{}", chain_id)).await {
            // Kiểm tra dữ liệu về rủi ro
            if data.get("is_scam").map_or(false, |v| v == "true") || 
               data.get("is_blacklisted").map_or(false, |v| v == "true") {
                is_risky = true;
            }
            
            // Lấy điểm audit nếu có
            if let Some(audit_score) = data.get("audit_score") {
                if let Ok(score) = audit_score.parse::<f64>() {
                    safety_score = Some(score);
                }
            }
            
            // Thu thập dữ liệu bổ sung
            all_data.extend(data);
        }
        
        // Gửi thông báo nếu phát hiện rủi ro từ các nguồn bên ngoài
        if is_risky {
            self.send_alert(
                token_address,
                "external_warning",
                "Cảnh báo: Token này đã được cộng đồng đánh dấu là nguy hiểm",
                4, 
                chain_id
            ).await;
        }
        
        (is_risky, safety_score, all_data)
    }
    
    /// Thực hiện phân tích an toàn đầy đủ cho token
    ///
    /// Gọi nhiều phương thức phân tích khác nhau để có cái nhìn toàn diện
    /// về độ an toàn của token.
    ///
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    /// - `adapter`: EVM adapter
    ///
    /// # Returns
    /// - `anyhow::Result<(bool, HashMap<String, String>, String)>`: (An toàn?, Kết quả phân tích chi tiết, Lý do tóm tắt)
    pub async fn full_safety_analysis(
        &self, 
        chain_id: u32, 
        token_address: &str, 
        adapter: &Arc<EvmAdapter>
    ) -> anyhow::Result<(bool, HashMap<String, String>, String)> {
        let mut results = HashMap::new();
        let mut issues = Vec::new();
        
        // 1. Kiểm tra contract info cơ bản
        match self.get_contract_info(chain_id, token_address, adapter).await {
            Some(contract_info) => {
                // Thông tin cơ bản
                results.insert("name".to_string(), contract_info.name.clone());
                results.insert("symbol".to_string(), contract_info.symbol.clone());
                results.insert("total_supply".to_string(), contract_info.total_supply.to_string());
                results.insert("verified".to_string(), contract_info.is_verified.to_string());
                
                // Kiểm tra hợp đồng xác minh
                if !contract_info.is_verified {
                    issues.push("Contract chưa được xác minh".to_string());
                }
            },
            None => {
                // Không tìm thấy contract info - rủi ro cao
                issues.push("Không thể lấy thông tin contract".to_string());
                results.insert("contract_info".to_string(), "error".to_string());
                
                // Trả về sớm vì không thể phân tích thêm
                let summary = "Token không an toàn: Không thể lấy thông tin contract".to_string();
                return Ok((false, results, summary));
            }
        }
        
        // 2. Phân tích rủi ro
        let risk_analysis = self.analyze_security_risk(chain_id, token_address, adapter).await?;
        let (risk_score, risk_factors, recommendation) = risk_analysis;
        
        results.insert("risk_score".to_string(), risk_score.to_string());
        results.insert("recommendation".to_string(), format!("{:?}", recommendation));
        
        // Thêm các yếu tố rủi ro vào issues
        for factor in risk_factors {
            match factor {
                RiskFactor::Honeypot(_) => issues.push("Token có dấu hiệu honeypot".to_string()),
                RiskFactor::HighTax(score) => issues.push(format!("Tax cao (score: {:.1})", score)),
                RiskFactor::LowLiquidity(_) => issues.push("Thanh khoản thấp".to_string()),
                RiskFactor::OwnerPrivileges(score) => issues.push(format!("Owner có quyền nguy hiểm (score: {:.1})", score)),
                RiskFactor::ContractStructure(score) => issues.push(format!("Cấu trúc contract đáng ngờ (score: {:.1})", score)),
                RiskFactor::BytecodeIssues(score) => issues.push(format!("Vấn đề bytecode (score: {:.1})", score)),
                RiskFactor::ContractUnavailable(_) => issues.push("Contract không khả dụng".to_string()),
                _ => {}
            }
        }
        
        // 3. Kiểm tra khả năng giao dịch
        let tradability_check = self.check_token_tradability(chain_id, token_address, adapter).await;
        let (non_tradable, reason) = tradability_check;
        
        results.insert("tradable".to_string(), (!non_tradable).to_string());
        
        if non_tradable {
            issues.push(format!("Token không thể giao dịch: {}", reason));
        }
        
        // 4. Kiểm tra tax
        match adapter.get_token_tax_info(token_address).await {
            Ok((buy_tax, sell_tax)) => {
                results.insert("buy_tax".to_string(), format!("{:.2}%", buy_tax));
                results.insert("sell_tax".to_string(), format!("{:.2}%", sell_tax));
                
                if buy_tax > MAX_SAFE_BUY_TAX {
                    issues.push(format!("Buy tax cao: {:.2}%", buy_tax));
                }
                
                if sell_tax > MAX_SAFE_SELL_TAX {
                    issues.push(format!("Sell tax cao: {:.2}%", sell_tax));
                }
                
                if (sell_tax - buy_tax).abs() > DANGEROUS_TAX_DIFF {
                    issues.push(format!("Chênh lệch tax đáng ngờ: Buy {:.2}%, Sell {:.2}%", buy_tax, sell_tax));
                }
            },
            Err(e) => {
                warn!("Không thể lấy thông tin tax cho {}: {}", token_address, e);
                results.insert("tax_info".to_string(), "error".to_string());
                issues.push("Không thể lấy thông tin tax".to_string());
            }
        }
        
        // 5. Kiểm tra thanh khoản
        match adapter.get_token_liquidity(token_address).await {
            Ok(liquidity) => {
                results.insert("liquidity_usd".to_string(), format!("${:.2}", liquidity));
                
                if liquidity < MIN_LIQUIDITY_THRESHOLD {
                    issues.push(format!("Thanh khoản thấp: ${:.2}", liquidity));
                }
            },
            Err(e) => {
                warn!("Không thể lấy thông tin thanh khoản cho {}: {}", token_address, e);
                results.insert("liquidity_info".to_string(), "error".to_string());
                issues.push("Không thể lấy thông tin thanh khoản".to_string());
            }
        }
        
        // 6. Kiểm tra giao dịch gần đây
        match adapter.get_token_transaction_history(token_address, 20).await {
            Ok(history) => {
                let (buy_count, sell_count) = self.analyze_transaction_pattern(&history);
                results.insert("recent_buys".to_string(), buy_count.to_string());
                results.insert("recent_sells".to_string(), sell_count.to_string());
                
                // Đánh giá mẫu giao dịch
                if buy_count > 0 && sell_count == 0 {
                    issues.push("Giao dịch một chiều: Chỉ có mua, không có bán".to_string());
                }
            },
            Err(e) => {
                warn!("Không thể lấy lịch sử giao dịch cho {}: {}", token_address, e);
                results.insert("transaction_history".to_string(), "error".to_string());
            }
        }
        
        // 7. Phân tích từ cộng đồng (nếu có)
        let (has_community_data, _, community_insights) = self.crowd_sourced_analysis(token_address, chain_id).await;
        
        if has_community_data {
            // Thêm dữ liệu từ cộng đồng vào kết quả
            for (key, value) in community_insights {
                results.insert(format!("community_{}", key), value);
            }
        }
        
        // Tạo tóm tắt
        let is_safe = issues.is_empty() || (issues.len() == 1 && issues[0].contains("xác minh"));
        let summary = if is_safe {
            "Token an toàn: Không phát hiện vấn đề nghiêm trọng".to_string()
        } else {
            format!("Token không an toàn: {}", issues.join(", "))
        };
        
        // Thêm tóm tắt vào kết quả
        results.insert("issues_count".to_string(), issues.len().to_string());
        for (i, issue) in issues.iter().enumerate() {
            results.insert(format!("issue_{}", i), issue.clone());
        }
        
        Ok((is_safe, results, summary))
    }
}
