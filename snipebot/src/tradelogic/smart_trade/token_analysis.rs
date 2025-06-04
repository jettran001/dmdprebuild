/// Token analysis implementation for SmartTradeExecutor
///
/// This module contains functions for analyzing token safety, detecting
/// honeypot, dynamic tax, liquidity risk, and other potential threats.
///
/// # Kiến trúc phân tích token dựa trên traits
/// - Module này sử dụng TokenAnalysisProvider trait từ analys/api/token_api.rs
/// - Tuân theo nguyên tắc trait-based design từ .cursorrc
/// - Tương tác thông qua chức năng trừu tượng được định nghĩa trong trait
/// - Mở rộng chức năng cơ bản của TokenStatusAnalyzer với các phân tích chuyên biệt
///
/// # Các tính năng chính:
/// - Áp dụng xử lý lỗi chuẩn với anyhow::Result
/// - Đảm bảo thread safety trong các phương thức async
/// - Tối ưu hóa các truy vấn blockchain song song
/// - Tuân thủ nghiêm ngặt quy tắc từ .cursorrc
/// - Sử dụng common/analysis.rs cho các hàm phân tích chung

// External imports
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::{Result, Context, anyhow};
use tracing::{debug, error, info, warn};

// Internal imports
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::analys::token_status::{
    TokenStatusAnalyzer, TokenStatus, TokenSafety, ContractInfo, 
    LiquidityEvent, LiquidityEventType, AdvancedTokenAnalysis
};

// Module imports
use super::types::{TradeTracker, TradeStatus};
use super::constants::*;
use super::executor::SmartTradeExecutor;

// Use common analysis functions
use crate::tradelogic::common::types::{TokenIssue, SecurityCheckResult};
use crate::tradelogic::common::analysis::{
    detect_token_issues,
    is_proxy_contract, 
    has_blacklist_function,
    has_excessive_privileges,
    analyze_token_security,
    convert_token_safety_to_security_check
};

impl SmartTradeExecutor {
    /// Phát hiện honeypot token
    ///
    /// Kiểm tra xem token có cho phép bán hay không
    ///
    /// # Arguments
    /// * `chain_id` - ID của blockchain
    /// * `token_address` - Địa chỉ token
    /// * `adapter` - EVM adapter
    ///
    /// # Returns
    /// * `bool` - True nếu token là honeypot
    pub async fn detect_honeypot(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> bool {
        // Sử dụng analys_client để phân tích token
        if let Ok(safety) = self.analys_client.token_provider()
            .analyze_token(chain_id, token_address).await {
            
            // Kiểm tra honeypot flag từ kết quả phân tích
            if let Some(is_honeypot) = safety.is_honeypot {
                if is_honeypot {
                    info!("Token {} phát hiện là honeypot!", token_address);
                    return true;
                }
            }
            
            // Nếu không có flag rõ ràng, thử simulate sell
            match adapter.simulate_sell_token(token_address, 1.0).await {
                Ok(_) => {
                    debug!("Token {} có thể bán (không phải honeypot)", token_address);
                    false
                }
                Err(e) => {
                    warn!("Không thể simulate bán token {}: {}. Có thể là honeypot.", token_address, e);
                    true
                }
            }
        } else {
            // Không thể phân tích, thử simulate sell
            match adapter.simulate_sell_token(token_address, 1.0).await {
                Ok(_) => false,
                Err(_) => {
                    warn!("Không thể simulate bán token {}. Có thể là honeypot.", token_address);
                    true
                }
            }
        }
    }

    /// Phát hiện thuế động (dynamic tax)
    ///
    /// Phát hiện tax thay đổi theo thời gian hoặc số lượng giao dịch
    ///
    /// # Arguments
    /// * `chain_id` - ID của blockchain
    /// * `token_address` - Địa chỉ token
    /// * `adapter` - EVM adapter
    ///
    /// # Returns
    /// * `(bool, f64, f64)` - (Có thuế động?, buy tax, sell tax)
    pub async fn detect_dynamic_tax(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> (bool, f64, f64) {
        // Sử dụng token provider từ analys_client
        if let Ok(tax_info) = self.analys_client.token_provider()
            .get_token_tax_info(chain_id, token_address).await {
            
            let buy_tax = tax_info.buy_tax.unwrap_or(0.0);
            let sell_tax = tax_info.sell_tax.unwrap_or(0.0);
            let is_dynamic = tax_info.is_dynamic_tax.unwrap_or(false);
            
            // Log kết quả
            if is_dynamic {
                warn!("Phát hiện thuế động trên token {}: buy_tax={}, sell_tax={}", token_address, buy_tax, sell_tax);
            } else {
                debug!("Token {} có thuế cố định: buy_tax={}, sell_tax={}", token_address, buy_tax, sell_tax);
            }
            
            return (is_dynamic, buy_tax, sell_tax);
        }
        
        // Nếu không thể lấy thông tin từ provider, thử cách khác
        // Thực hiện hai giao dịch mua khác nhau để đánh giá thuế
        debug!("Thử nghiệm thuế bằng cách simulate giao dịch mua với số lượng khác nhau");
        
        let small_amount = 0.01; // 0.01 ETH/BNB
        let large_amount = 0.5;  // 0.5 ETH/BNB
        
        // Simulate mua với số lượng nhỏ
        let (small_out, _) = adapter.simulate_swap_amount_out(
            "ETH", token_address, small_amount
        ).await.unwrap_or((0.0, 0.0));
        
        // Simulate mua với số lượng lớn
        let (large_out, _) = adapter.simulate_swap_amount_out(
            "ETH", token_address, large_amount
        ).await.unwrap_or((0.0, 0.0));
        
        // Tính tỷ lệ output/input và so sánh
        let small_ratio = if small_amount > 0.0 { small_out / small_amount } else { 0.0 };
        let large_ratio = if large_amount > 0.0 { large_out / large_amount } else { 0.0 };
        
        // Tính chênh lệch phần trăm
        let difference = if small_ratio > 0.0 {
            ((large_ratio - small_ratio) / small_ratio).abs() * 100.0
        } else {
            0.0
        };
        
        // Nếu chênh lệch > 3%, có thể là thuế động
        let is_dynamic = difference > 3.0;
        if is_dynamic {
            warn!("Phát hiện thuế động trên token {}: chênh lệch {}%", token_address, difference);
        }
        
        // Ước tính thuế
        let estimated_tax = if small_ratio > 0.0 {
            (1.0 - (small_ratio / large_ratio)) * 100.0
        } else {
            0.0
        };
        
        (is_dynamic, estimated_tax, estimated_tax)
    }

    /// Phân tích token toàn diện 
    ///
    /// Kết hợp phân tích từ common/analysis và analys_client
    /// 
    /// # Arguments
    /// * `chain_id` - Chain ID
    /// * `token_address` - Token address
    /// 
    /// # Returns
    /// * `Result<SecurityCheckResult>` - Standardized security check result
    pub async fn analyze_token_comprehensive(&self, chain_id: u32, token_address: &str) -> Result<SecurityCheckResult> {
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Err(anyhow!("No adapter found for chain ID {}", chain_id)),
        };
        
        // Sử dụng analyze_token_security từ common/analysis.rs
        let base_result = analyze_token_security(token_address, adapter).await?;
        
        // Bổ sung phân tích từ analys_client
        if let Ok(safety) = self.analys_client.token_provider()
            .analyze_token(chain_id, token_address).await {
            
            // Chuyển đổi và kết hợp kết quả
            let client_result = convert_token_safety_to_security_check(&safety, token_address);
            
            // Kết hợp các vấn đề từ cả hai nguồn
            let mut combined_issues = base_result.issues;
            combined_issues.extend(client_result.issues);
            
            // Sử dụng risk score cao hơn
            let risk_score = base_result.risk_score.max(client_result.risk_score);
            
            // Tạo kết quả kết hợp
            return Ok(SecurityCheckResult {
                token_address: token_address.to_string(),
                issues: combined_issues,
                risk_score,
                safe_to_trade: risk_score < 70,
                extra_info: base_result.extra_info,
            });
        }
        
        // Nếu không thể lấy từ analys_client, trả về kết quả cơ bản
        Ok(base_result)
    }

    /// Phát hiện thanh khoản thấp hoặc rút thanh khoản đột ngột
    pub async fn detect_liquidity_risk(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> (bool, String) {
        let token_analyzer = match self.get_token_analyzer().await {
            Some(analyzer) => analyzer,
            None => {
                return self.fallback_liquidity_detection(token_address, adapter).await;
            }
        };
        
        // Kiểm tra thanh khoản và sự kiện
        match token_analyzer.analyze_liquidity_risk(token_address, chain_id).await {
            Ok((has_risk, reason)) => (has_risk, reason),
            Err(e) => {
                warn!("Lỗi khi phân tích rủi ro thanh khoản: {}", e);
                self.fallback_liquidity_detection(token_address, adapter).await
            }
        }
    }
    
    /// Phương pháp dự phòng kiểm tra thanh khoản
    async fn fallback_liquidity_detection(&self, token_address: &str, adapter: &Arc<EvmAdapter>) -> (bool, String) {
        match adapter.get_token_liquidity(token_address).await {
            Ok(liquidity) => {
                if liquidity < MIN_LIQUIDITY_THRESHOLD {
                    (true, format!("Thanh khoản thấp: ${:.2}", liquidity))
                } else {
                    (false, "Thanh khoản ổn định".to_string())
                }
            },
            Err(_) => {
                (true, "Không thể kiểm tra thanh khoản".to_string())
            }
        }
    }

    /// Phân tích quyền của owner contract
    pub async fn detect_owner_privilege(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> (bool, Vec<String>) {
        // Lấy thông tin contract
        let contract_info_result = self.get_contract_info(chain_id, token_address, adapter).await;
        
        if let Some(contract_info) = contract_info_result {
            // Sử dụng TokenStatus để phân tích
            let token_status = TokenStatus::new();
            let owner_analysis = token_status.analyze_owner_privileges(&contract_info);
            
            // Danh sách quyền hạn đáng ngại
            let mut dangerous_privileges = Vec::new();
            
            // Kiểm tra các quyền nguy hiểm
            if owner_analysis.has_mint_authority {
                dangerous_privileges.push("Mint vô hạn".to_string());
            }
            
            if owner_analysis.has_pause_authority {
                dangerous_privileges.push("Dừng giao dịch".to_string());
            }
            
            if owner_analysis.has_burn_authority {
                dangerous_privileges.push("Burn token bất kỳ".to_string());
            }
            
            if owner_analysis.can_retrieve_ownership {
                dangerous_privileges.push("Lấy lại quyền owner sau khi từ bỏ".to_string());
            }
            
            if owner_analysis.is_proxy {
                dangerous_privileges.push("Contract proxy có thể nâng cấp".to_string());
            }
            
            // Phát hiện các hàm nguy hiểm
            let dangerous_functions = token_status.detect_dangerous_functions(&contract_info);
            for func in dangerous_functions {
                dangerous_privileges.push(func);
            }
            
            // Kết luận dựa trên số lượng quyền nguy hiểm
            let is_dangerous = !dangerous_privileges.is_empty();
            
            (is_dangerous, dangerous_privileges)
        } else {
            // Không lấy được thông tin contract, nguy hiểm
            (true, vec!["Không thể phân tích contract".to_string()])
        }
    }
    
    /// Phát hiện cơ chế blacklist/whitelist
    pub async fn detect_blacklist(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> bool {
        // Lấy thông tin contract
        let contract_info_result = self.get_contract_info(chain_id, token_address, adapter).await;
        
        if let Some(contract_info) = contract_info_result {
            // Kiểm tra source code có blacklist không
            if let Some(source_code) = &contract_info.source_code {
                // Các pattern blacklist phổ biến
                let blacklist_patterns = [
                    "blacklist", "blocklist", "blacklisted", "banned", 
                    "isBlacklisted", "_blacklist", "denylist", "blackList"
                ];
                
                // Tìm pattern trong source code
                for pattern in blacklist_patterns.iter() {
                    if source_code.to_lowercase().contains(&pattern.to_lowercase()) {
                        return true;
                    }
                }
            }
            
            false
        } else {
            // Không lấy được thông tin contract, nguy hiểm
            true
        }
    }
    
    /// Phát hiện contract là proxy hoặc upgradeable
    /// 
    /// Kiểm tra xem contract có thể được nâng cấp logic sau khi launch hay không
    /// 
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    /// - `adapter`: EVM adapter
    /// 
    /// # Returns
    /// - `bool`: True nếu là proxy contract
    /// - `bool`: True nếu owner có quyền upgrade
    pub async fn detect_proxy(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> (bool, bool) {
        if let Some(contract_info) = self.get_contract_info(chain_id, token_address, adapter).await {
            // Sử dụng TokenStatus để phân tích contract
            let token_status = TokenStatus::new();
            
            // Kiểm tra có phải là proxy không
            let is_proxy = token_status.is_proxy_contract(&contract_info);
            
            // Kiểm tra quyền upgrade
            let owner_analysis = token_status.analyze_owner_privileges(&contract_info);
            let owner_can_upgrade = is_proxy && !owner_analysis.is_ownership_renounced;
            
            self.log_trade_decision(
                "N/A", 
                token_address, 
                TradeStatus::Pending, 
                &format!("Proxy check: is_proxy={}, owner_can_upgrade={}", is_proxy, owner_can_upgrade),
                chain_id
            ).await;
            
            return (is_proxy, owner_can_upgrade);
        }
        
        (false, false)
    }

    /// Phát hiện external call hoặc delegatecall
    /// 
    /// Kiểm tra xem contract có gọi external code/delegatecall không (có thể gây rug)
    /// 
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    /// - `adapter`: EVM adapter
    /// 
    /// # Returns
    /// - `bool`: True nếu có external call hoặc delegatecall
    /// - `Vec<String>`: Danh sách các hàm nguy hiểm được phát hiện
    pub async fn detect_external_call(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> (bool, Vec<String>) {
        if let Some(contract_info) = self.get_contract_info(chain_id, token_address, adapter).await {
            // Sử dụng TokenStatus để phân tích bytecode
            let token_status = TokenStatus::new();
            
            // Kiểm tra external call/delegatecall
            let has_external_calls = token_status.has_external_delegatecall(&contract_info);
            
            // Lấy danh sách các hàm nguy hiểm
            let bytecode_analysis = token_status.analyze_bytecode(&contract_info);
            let dangerous_functions = bytecode_analysis.dangerous_functions.clone();
            
            self.log_trade_decision(
                "N/A", 
                token_address, 
                TradeStatus::Pending, 
                &format!("External call check: has_external_calls={}, dangerous_functions={:?}", 
                         has_external_calls, dangerous_functions),
                chain_id
            ).await;
            
            return (has_external_calls, dangerous_functions);
        }
        
        (false, Vec::new())
    }

    /// Phát hiện anti-bot và anti-whale
    /// 
    /// Kiểm tra xem token có giới hạn giao dịch hoặc giới hạn số lượng token trong ví không
    /// 
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    /// - `adapter`: EVM adapter
    /// 
    /// # Returns
    /// - `bool`: True nếu có giới hạn giao dịch (anti-bot)
    /// - `bool`: True nếu có giới hạn số token một ví có thể giữ (anti-whale)
    /// - `bool`: True nếu có cooldown giữa các giao dịch
    pub async fn detect_anti_bot(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> (bool, bool, bool) {
        if let Some(contract_info) = self.get_contract_info(chain_id, token_address, adapter).await {
            let token_status = TokenStatus::new();
            
            // Kiểm tra max transaction limit
            let has_tx_limit = token_status.has_max_tx_or_wallet_limit(&contract_info);
            
            // Kiểm tra trading cooldown
            let has_cooldown = token_status.has_trading_cooldown(&contract_info);
            
            // Phân tích nâng cao từ code
            let advanced_analysis = token_status.analyze_code_without_api(&contract_info);
            
            // Anti-whale là giới hạn số lượng token trong wallet
            let has_anti_whale = advanced_analysis.has_anti_whale;
            
            self.log_trade_decision(
                "N/A", 
                token_address, 
                TradeStatus::Pending, 
                &format!("Anti-bot check: has_tx_limit={}, has_anti_whale={}, has_cooldown={}", 
                         has_tx_limit, has_anti_whale, has_cooldown),
                chain_id
            ).await;
            
            return (has_tx_limit, has_anti_whale, has_cooldown);
        }
        
        (false, false, false)
    }
    
    /// Kiểm tra xem quyền sở hữu có được từ bỏ thực sự không
    ///
    /// Phân tích sâu hơn để kiểm tra quyền sở hữu đã được từ bỏ thật sự chưa
    /// hoặc còn tồn tại backdoor để lấy lại quyền
    /// 
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    /// - `adapter`: EVM adapter
    /// 
    /// # Returns
    /// - `bool`: True nếu quyền sở hữu đã thực sự được từ bỏ
    /// - `bool`: True nếu phát hiện backdoor để lấy lại quyền
    pub async fn is_ownership_renounced(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> (bool, bool) {
        if let Some(contract_info) = self.get_contract_info(chain_id, token_address, adapter).await {
            // Tạo đối tượng TokenStatus để phân tích
            let token_status = TokenStatus::new();

            // Kiểm tra owner đã từ bỏ quyền chưa
            let is_renounced = token_status.is_ownership_renounced(&contract_info);
            
            // Phân tích quyền hạn owner để kiểm tra backdoor
            let owner_analysis = token_status.analyze_owner_privileges(&contract_info);
            let has_backdoor = owner_analysis.can_retrieve_ownership;
            
            self.log_trade_decision(
                "N/A", 
                token_address, 
                TradeStatus::Pending, 
                &format!("Ownership check: renounced={}, has_backdoor={}", is_renounced, has_backdoor),
                chain_id
            ).await;
            
            return (is_renounced, has_backdoor);
        }
        
        (false, false)
    }

    /// Phát hiện token có trading cooldown không
    /// 
    /// Kiểm tra nếu token có giới hạn thời gian giữa các giao dịch (anti-bot)
    /// 
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    /// - `adapter`: EVM adapter
    /// 
    /// # Returns
    /// - `bool`: True nếu có trading cooldown
    /// - `u64`: Thời gian cooldown (giây), 0 nếu không có
    pub async fn detect_trading_cooldown(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> (bool, u64) {
        if let Some(contract_info) = self.get_contract_info(chain_id, token_address, adapter).await {
            // Tạo đối tượng TokenStatus để phân tích
            let token_status = TokenStatus::new();

            // Kiểm tra có trading cooldown không
            let has_cooldown = token_status.has_trading_cooldown(&contract_info);
            
            // Nếu có cooldown, tìm thời gian cụ thể
            let cooldown_seconds = if has_cooldown {
                // Phân tích mã nguồn để tìm thời gian cooldown
                if let Some(source_code) = &contract_info.source_code {
                    // Tìm các pattern thời gian cooldown phổ biến
                    if let Some(cooldown_match) = Regex::new(r"(?i)cooldown\s*=\s*(\d+)")
                        .ok()
                        .and_then(|re| re.captures(source_code))
                        .and_then(|cap| cap.get(1))
                    {
                        cooldown_match.as_str().parse::<u64>().unwrap_or(60)
                    } else {
                        // Default cooldown nếu không tìm thấy giá trị cụ thể
                        60
                    }
                } else {
                    60
                }
            } else {
                0
            };
            
            self.log_trade_decision(
                "N/A", 
                token_address, 
                TradeStatus::Pending, 
                &format!("Trading cooldown check: has_cooldown={}, seconds={}", has_cooldown, cooldown_seconds),
                chain_id
            ).await;
            
            return (has_cooldown, cooldown_seconds);
        }
        
        (false, 0)
    }

    /// Phát hiện giới hạn giao dịch và giới hạn ví
    /// 
    /// Kiểm tra nếu token có giới hạn số lượng tối đa cho mỗi giao dịch
    /// hoặc giới hạn số lượng token trong ví
    /// 
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    /// - `adapter`: EVM adapter
    /// 
    /// # Returns
    /// - `bool`: True nếu có giới hạn giao dịch
    /// - `bool`: True nếu có giới hạn ví
    /// - `f64`: Phần trăm của tổng cung có thể giao dịch trong một lần, 0 nếu không giới hạn
    pub async fn detect_transaction_limits(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> (bool, bool, f64) {
        if let Some(contract_info) = self.get_contract_info(chain_id, token_address, adapter).await {
            // Tạo đối tượng TokenStatus để phân tích
            let token_status = TokenStatus::new();

            // Kiểm tra có giới hạn giao dịch hay không
            let has_tx_limit = token_status.has_max_tx_or_wallet_limit(&contract_info);
            
            // Tìm chi tiết về giới hạn
            let (has_wallet_limit, max_tx_percent) = if has_tx_limit {
                // Phân tích mã nguồn để tìm giới hạn cụ thể
                if let Some(source_code) = &contract_info.source_code {
                    // Tìm các mẫu giới hạn ví
                    let wallet_limit_pattern = Regex::new(r"(?i)(maxWallet|maxHolding|maxBalance)").ok();
                    let has_wallet_limit = wallet_limit_pattern
                        .as_ref()
                        .map(|re| re.is_match(source_code))
                        .unwrap_or(false);
                    
                    // Tìm phần trăm giới hạn giao dịch
                    let tx_percent = Regex::new(r"(?i)_max(Transaction|Tx|TxAmount)\s*=\s*(\d+)")
                        .ok()
                        .and_then(|re| re.captures(source_code))
                        .and_then(|cap| cap.get(2))
                        .and_then(|m| m.as_str().parse::<f64>().ok())
                        .unwrap_or(1.0);
                    
                    (has_wallet_limit, tx_percent)
                } else {
                    (false, 1.0)
                }
            } else {
                (false, 0.0)
            };
            
            self.log_trade_decision(
                "N/A", 
                token_address, 
                TradeStatus::Pending, 
                &format!("Transaction limits: has_tx_limit={}, has_wallet_limit={}, max_tx_percent={}%", 
                         has_tx_limit, has_wallet_limit, max_tx_percent),
                chain_id
            ).await;
            
            return (has_tx_limit, has_wallet_limit, max_tx_percent);
        }
        
        (false, false, 0.0)
    }

    /// Phát hiện tax hoặc phí ẩn
    /// 
    /// Kiểm tra nếu token có tax hoặc phí ẩn cao hơn mức ghi trong smart contract
    /// 
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    /// - `adapter`: EVM adapter
    /// 
    /// # Returns
    /// - `bool`: True nếu phát hiện tax ẩn
    /// - `f64`: Tax/phí thực tế (%)
    /// - `f64`: Tax/phí được công bố (%)
    pub async fn detect_hidden_fees(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> (bool, f64, f64) {
        if let Some(contract_info) = self.get_contract_info(chain_id, token_address, adapter).await {
            // Tạo đối tượng TokenStatus để phân tích
            let token_status = TokenStatus::new();

            // Kiểm tra có hidden fees không
            let has_hidden_fees = token_status.has_hidden_fees(&contract_info);
            
            // Lấy tax từ hợp đồng và tax thực tế
            let declared_tax = if let Some(source_code) = &contract_info.source_code {
                // Tìm các pattern tax phổ biến
                Regex::new(r"(?i)(fee|tax)\s*=\s*(\d+)")
                    .ok()
                    .and_then(|re| re.captures(source_code))
                    .and_then(|cap| cap.get(2))
                    .and_then(|m| m.as_str().parse::<f64>().ok())
                    .unwrap_or(0.0)
            } else {
                0.0
            };
            
            // Tính tax thực tế bằng cách thử giao dịch nhỏ
            let (actual_tax, _) = adapter.simulate_buy_sell_slippage(token_address, 0.01).await.unwrap_or((0.0, 0.0));
            
            // Tax ẩn nếu thực tế cao hơn khai báo >2%
            let has_hidden = (actual_tax - declared_tax).abs() > 2.0;
            
            self.log_trade_decision(
                "N/A", 
                token_address, 
                TradeStatus::Pending, 
                &format!("Hidden fees check: has_hidden={}, declared={}%, actual={}%", 
                         has_hidden, declared_tax, actual_tax),
                chain_id
            ).await;
            
            return (has_hidden, actual_tax, declared_tax);
        }
        
        (false, 0.0, 0.0)
    }

    /// Phân tích sự kiện thanh khoản để phát hiện rug pull
    /// 
    /// Phân tích lịch sử add/remove liquidity để phát hiện dấu hiệu rút LP bất thường
    /// 
    /// # Parameters
    /// - `chain_id`: ID của blockchain
    /// - `token_address`: Địa chỉ token
    /// - `adapter`: EVM adapter
    /// 
    /// # Returns
    /// - `bool`: True nếu phát hiện dấu hiệu rug pull
    /// - `String`: Chi tiết về các dấu hiệu phát hiện được
    pub async fn analyze_liquidity_events(&self, chain_id: u32, token_address: &str, adapter: &Arc<EvmAdapter>) -> (bool, String) {
        // Lấy lịch sử sự kiện thanh khoản
        if let Ok(liquidity_events) = adapter.get_liquidity_events(token_address, 50).await {
            // Tạo đối tượng TokenStatus để phân tích
            let token_status = TokenStatus::new();

            // Kiểm tra bất thường trong sự kiện thanh khoản
            let abnormal = token_status.abnormal_liquidity_events(&liquidity_events);
            
            // Phân tích chi tiết
            let mut details = String::new();
            let mut add_events = 0;
            let mut remove_events = 0;
            let mut total_added = 0.0;
            let mut total_removed = 0.0;
            
            for event in &liquidity_events {
                match event.event_type {
                    LiquidityEventType::AddLiquidity => {
                        add_events += 1;
                        if let Ok(amount) = event.base_amount.parse::<f64>() {
                            total_added += amount;
                        }
                    },
                    LiquidityEventType::RemoveLiquidity => {
                        remove_events += 1;
                        if let Ok(amount) = event.base_amount.parse::<f64>() {
                            total_removed += amount;
                        }
                    }
                }
            }
            
            // Tính % LP đã rút
            let percentage_removed = if total_added > 0.0 {
                (total_removed / total_added) * 100.0
            } else {
                0.0
            };
            
            // Tạo chi tiết phân tích
            details = format!(
                "Add events: {}, Remove events: {}, Added: {:.4} ETH/BNB, Removed: {:.4} ETH/BNB ({}%)",
                add_events, remove_events, total_added, total_removed, percentage_removed
            );
            
            // Xác định dấu hiệu rug pull
            let is_rugpull_pattern = abnormal || percentage_removed > 70.0;
            
            if is_rugpull_pattern {
                details = format!("⚠️ RUG PULL PATTERN! {}", details);
            }
            
            self.log_trade_decision(
                "N/A", 
                token_address, 
                TradeStatus::Pending, 
                &format!("Liquidity analysis: abnormal={}, details={}", abnormal, details),
                chain_id
            ).await;
            
            return (is_rugpull_pattern, details);
        }
        
        (false, "Không thể lấy dữ liệu sự kiện thanh khoản".to_string())
    }

    /// Lấy TokenStatusAnalyzer từ module analys
    async fn get_token_analyzer(&self) -> Option<Arc<TokenStatusAnalyzer>> {
        // Trong thực tế, đây có thể được khởi tạo một lần và lưu trong struct
        // Hoặc được truyền vào qua constructor
        Some(Arc::new(TokenStatusAnalyzer::new()))
    }

    /// Log quyết định giao dịch
    /// 
    /// # Arguments
    /// * `trade_id` - ID giao dịch
    /// * `token_address` - Địa chỉ token
    /// * `status` - Trạng thái giao dịch
    /// * `reason` - Lý do
    /// * `chain_id` - ID của chain
    /// 
    /// # Note
    /// Sử dụng implementation trong alert.rs để tránh trùng lặp
    async fn log_trade_decision(&self, trade_id: &str, token_address: &str, status: TradeStatus, reason: &str, chain_id: u32) {
        // Gọi đến implementation chính trong alert.rs
        // Chỉ log debug trong trường hợp implementation chính chưa sẵn sàng
        debug!(
            "Trade decision for token {}: Status={:?}, Chain={}, Reason: {}",
            token_address, status, chain_id, reason
        );
    }
}
