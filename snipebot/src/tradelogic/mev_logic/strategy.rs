//! Strategy cho MEV Logic
//!
//! Module này triển khai các chiến lược MEV, phát hiện và tận dụng
//! các cơ hội MEV từ mempool.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tokio::time::sleep;
use futures::future::join_all;
use tracing::{error, info};

use crate::analys::mempool::{
    MempoolAnalyzer, MempoolTransaction, TransactionType,
    SuspiciousPattern, MempoolAlert, AlertSeverity, NewTokenInfo, create_mempool_analyzer,
    MempoolAlertType,
};
use crate::analys::token_status::{TokenStatus, ContractInfo};
use crate::chain_adapters::evm_adapter::EvmAdapter;

use super::constants::*;
use super::types::*;
use super::traits::MevBot;

/// Chiến lược phát hiện và tận dụng MEV
pub struct MevStrategy {
    /// Mempool analyzer cho mỗi chain được theo dõi
    mempool_analyzers: HashMap<u32, Arc<MempoolAnalyzer>>,
    /// Các EVM adapter tương ứng
    evm_adapters: HashMap<u32, Arc<EvmAdapter>>,
    /// Cơ hội MEV đã phát hiện
    opportunities: RwLock<Vec<MevOpportunity>>,
    /// Cấu hình MEV
    config: RwLock<MevConfig>,
    /// Thời gian lần cuối thực thi
    last_execution: RwLock<Instant>,
    /// Số lần thực thi trong phút hiện tại
    executions_this_minute: RwLock<u32>,
    /// Đang chạy hay không
    running: RwLock<bool>,
    /// Cache cho thông tin contract
    contract_info_cache: RwLock<HashMap<String, ContractInfo>>,
}

impl MevStrategy {
    /// Tạo mới MEV strategy
    pub fn new() -> Self {
        Self {
            mempool_analyzers: HashMap::new(),
            evm_adapters: HashMap::new(),
            opportunities: RwLock::new(Vec::new()),
            config: RwLock::new(MevConfig::default()),
            last_execution: RwLock::new(Instant::now()),
            executions_this_minute: RwLock::new(0),
            running: RwLock::new(false),
            contract_info_cache: RwLock::new(HashMap::new()),
        }
    }
    
    /// Thêm chain để theo dõi
    pub async fn add_chain(&mut self, chain_id: u32, adapter: Arc<EvmAdapter>) {
        let mempool_analyzer = Arc::new(create_mempool_analyzer(chain_id));
        self.mempool_analyzers.insert(chain_id, mempool_analyzer);
        self.evm_adapters.insert(chain_id, adapter);
    }
    
    /// Cập nhật cấu hình
    pub async fn update_config(&self, config: MevConfig) {
        let mut current_config = self.config.write().await;
        *current_config = config;
    }
    
    /// Bắt đầu theo dõi MEV
    pub async fn start(&self) {
        let mut running = self.running.write().await;
        if *running {
            return;
        }
        
        *running = true;
        
        // Clone các thông tin cần thiết
        let self_clone = self.clone();
        
        // Khởi động task theo dõi
        tokio::spawn(async move {
            self_clone.monitor_loop().await;
        });
    }
    
    /// Dừng theo dõi MEV
    pub async fn stop(&self) {
        let mut running = self.running.write().await;
        *running = false;
    }
    
    /// Vòng lặp theo dõi chính
    async fn monitor_loop(&self) {
        while *self.running.read().await {
            // Thực hiện tác vụ song song thay vì tuần tự
            let scan_future = self.scan_mev_opportunities();
            let evaluate_future = self.evaluate_opportunities();
            
            // Chạy đồng thời các tác vụ không phụ thuộc nhau
            let (_, _) = tokio::join!(scan_future, evaluate_future);
            
            // Xử lý các cơ hội có thể thực hiện
            self.execute_opportunities().await;
            
            // Chạy phân tích nâng cao
            let futures = self.mempool_analyzers.iter().map(|(chain_id, analyzer)| 
                self.analyze_advanced_mempool_opportunities(*chain_id, analyzer));
            join_all(futures).await;
            
            // Ngủ một khoảng thời gian
            sleep(Duration::from_millis(MEV_SCAN_INTERVAL_MS)).await;
        }
    }
    
    /// Quét cơ hội MEV từ mempool
    async fn scan_mev_opportunities(&self) {
        // Chạy song song phân tích cho tất cả chains
        let futures = self.mempool_analyzers.iter().map(|(chain_id, analyzer)| {
            self.scan_chain_opportunities(*chain_id, analyzer)
        });
        join_all(futures).await;
    }
    
    /// Quét cơ hội MEV cho một chain cụ thể
    async fn scan_chain_opportunities(&self, chain_id: u32, analyzer: &Arc<MempoolAnalyzer>) {
        // Lấy giao dịch đồng thời với lấy cảnh báo
        let txs_future = analyzer.get_pending_transactions(100);
        let alerts_future = analyzer.get_alerts(Some(AlertSeverity::Medium));
        
        // Chỉ định kiểu dữ liệu để tránh lỗi E0282: type annotations needed
        let (pending_transactions, alerts): (Vec<MempoolTransaction>, Vec<MempoolAlert>) = tokio::join!(txs_future, alerts_future);
        
        // Phân tích giao dịch
        self.analyze_pending_transactions(chain_id, &pending_transactions).await;
        
        // Xử lý cảnh báo từ mempool analyzer
        for alert in alerts {
            match alert.alert_type {
                MempoolAlertType::NewToken => {
                    if let Some(token) = &alert.related_token {
                        let token_clone = token.clone();
                        let alert_clone = alert.clone();
                        let self_clone = self.clone();
                        tokio::spawn(async move {
                            self_clone.process_new_token(chain_id, token_clone, alert_clone).await;
                        });
                    }
                },
                MempoolAlertType::LiquidityAdded => {
                    let alert_clone = alert.clone();
                    let self_clone = self.clone();
                    tokio::spawn(async move {
                        self_clone.process_new_liquidity(chain_id, alert_clone).await;
                    });
                },
                MempoolAlertType::LiquidityRemoved => {
                    let alert_clone = alert.clone();
                    let self_clone = self.clone();
                    tokio::spawn(async move {
                        self_clone.process_liquidity_removal(chain_id, alert_clone).await;
                    });
                },
                MempoolAlertType::SuspiciousTransaction(pattern) => {
                    match pattern {
                        SuspiciousPattern::FrontRunning => {
                            let alert_clone = alert.clone();
                            let self_clone = self.clone();
                            tokio::spawn(async move {
                                self_clone.process_front_running_opportunity(chain_id, alert_clone).await;
                            });
                        },
                        SuspiciousPattern::SandwichAttack => {
                            let alert_clone = alert.clone();
                            let self_clone = self.clone();
                            tokio::spawn(async move {
                                self_clone.process_sandwich_opportunity(chain_id, alert_clone).await;
                            });
                        },
                        SuspiciousPattern::WhaleMovement => {
                            let alert_clone = alert.clone();
                            let self_clone = self.clone();
                            tokio::spawn(async move {
                                self_clone.process_whale_transaction(chain_id, alert_clone).await;
                            });
                        },
                        _ => {}
                    }
                },
                MempoolAlertType::WhaleTransaction => {
                    let alert_clone = alert.clone();
                    let self_clone = self.clone();
                    tokio::spawn(async move {
                        self_clone.process_whale_transaction(chain_id, alert_clone).await;
                    });
                },
                _ => {}
            }
        }
    }
    
    /// Phân tích trực tiếp các giao dịch đang chờ trong mempool
    async fn analyze_pending_transactions(&self, chain_id: u32, transactions: &Vec<MempoolTransaction>) {
        let config = self.config.read().await;
        if !config.enabled {
            return;
        }

        // Nhóm các giao dịch theo loại
        let mut swaps = Vec::new();
        let mut liquidity_events = Vec::new();
        let mut token_approvals = Vec::new();
        let mut high_value_transfers = Vec::new();
        
        for tx in transactions {
            match tx.transaction_type {
                TransactionType::Swap => swaps.push(tx),
                TransactionType::AddLiquidity | TransactionType::RemoveLiquidity => liquidity_events.push(tx),
                TransactionType::Approve => token_approvals.push(tx),
                TransactionType::Transfer => {
                    if tx.value_usd > HIGH_VALUE_TX_ETH as f64 * tx.eth_price_usd {
                        high_value_transfers.push(tx);
                    }
                },
                _ => {}
            }
        }
        
        // Tìm kiếm mẫu giao dịch liên quan đến:
        
        // 1. Xác định các cặp token được swap nhiều lần - có thể là arbitrage
        if swaps.len() >= 2 {
            self.detect_potential_arbitrage_paths(chain_id, &swaps).await;
        }
        
        // 2. Phát hiện các giao dịch swap lớn có thể sandwich
        let large_swaps: Vec<&MempoolTransaction> = swaps.iter()
            .filter(|tx| tx.value_usd > MIN_TX_VALUE_ETH as f64 * tx.eth_price_usd)
            .collect();
        
        if !large_swaps.is_empty() {
            self.detect_sandwich_targets(chain_id, &large_swaps).await;
        }
        
        // 3. Phát hiện thêm/rút thanh khoản
        if !liquidity_events.is_empty() {
            self.analyze_liquidity_events(chain_id, &liquidity_events).await;
        }
        
        // 4. Phân tích chuỗi approve -> swap tiềm năng
        if !token_approvals.is_empty() && !swaps.is_empty() {
            self.correlate_approvals_and_swaps(chain_id, &token_approvals, &swaps).await;
        }
    }
    
    /// Tạo thông tin contract cho phân tích token
    async fn create_contract_info(&self, chain_id: u32, token_address: &str, token_info: &NewTokenInfo, adapter: &EvmAdapter) -> ContractInfo {
        // Tạo key để tìm trong cache
        let cache_key = format!("contract_info:{}:{}", chain_id, token_address);
        
        // Kiểm tra cache trước
        {
            let cache = self.contract_info_cache.read().await;
            if let Some(cached_info) = cache.get(&cache_key) {
                return cached_info.clone();
            }
        }
        
        // Tạo futures cho các truy vấn RPC song song
        let source_code_future = adapter.get_contract_source_code(token_address);
        let bytecode_future = adapter.get_contract_bytecode(token_address);
        
        // Thực hiện song song các truy vấn không phụ thuộc nhau
        let ((source_code, is_verified), bytecode) = tokio::join!(
            source_code_future,
            bytecode_future
        );
        
        // Lấy kết quả hoặc dùng giá trị mặc định nếu lỗi
        let (source_code, is_verified) = source_code.unwrap_or((None, false));
        let bytecode = bytecode.unwrap_or(None);
        
        // Chỉ lấy ABI nếu contract đã được xác minh
        let abi = if is_verified { 
            adapter.get_contract_abi(token_address).await.unwrap_or(None) 
        } else { 
            None 
        };
        
        // Lấy thông tin chủ sở hữu
        let owner_address = adapter.get_contract_owner(token_address).await.unwrap_or(None);
        
        let contract_info = ContractInfo {
            address: token_address.to_string(),
            chain_id,
            source_code,
            bytecode,
            abi,
            is_verified,
            owner_address,
        };
        
        // Lưu vào cache
        {
            let mut cache = self.contract_info_cache.write().await;
            cache.insert(cache_key, contract_info.clone());
        }
        
        contract_info
    }

    /// Phân tích an toàn của token
    async fn analyze_token_safety(&self, contract_info: ContractInfo) -> TokenStatus {
        // Tạo đối tượng để phân tích
        let token_status = TokenStatus::from_contract_info(
            &contract_info,
            &[], // Không có dữ liệu lịch sử thanh khoản lúc đầu
        );
        
        token_status
    }
    
    /// Lấy danh sách cơ hội hiện tại
    pub async fn get_opportunities(&self) -> Vec<MevOpportunity> {
        let opportunities = self.opportunities.read().await;
        opportunities.clone()
    }
    
    /// Lấy danh sách cơ hội hiện tại cho client sử dụng
    /// Đã sửa để trả về Vec được clone thay vì tham chiếu để tránh lỗi lifetime
    pub async fn get_opportunities_ref(&self) -> Vec<MevOpportunity> {
        let opportunities = self.opportunities.read().await;
        opportunities.clone()
    }
    
    /// Lấy cấu hình hiện tại
    pub async fn get_config(&self) -> MevConfig {
        let config = self.config.read().await;
        config.clone()
    }
    
    /// Lấy cấu hình hiện tại (trả về tham chiếu nếu khách hàng có thể xử lý)
    pub async fn get_config_ref<'a>(&'a self) -> tokio::sync::RwLockReadGuard<'a, MevConfig> {
        self.config.read().await
    }
    
    /// Clone cho async
    fn clone(&self) -> Self {
        Self {
            mempool_analyzers: self.mempool_analyzers.clone(),
            evm_adapters: self.evm_adapters.clone(),
            opportunities: RwLock::new(Vec::new()),
            config: RwLock::new(MevConfig::default()),
            last_execution: RwLock::new(Instant::now()),
            executions_this_minute: RwLock::new(0),
            running: RwLock::new(false),
            contract_info_cache: RwLock::new(HashMap::new()),
        }
    }

    /// Đánh giá các cơ hội đã phát hiện
    async fn evaluate_opportunities(&self) {
        // Lấy danh sách cơ hội hiện tại
        let mut opportunities = self.opportunities.write().await;

        // Kiểm tra cơ hội nào đã hết hạn và loại bỏ
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        opportunities.retain(|opp| {
            // Sử dụng expires_at thay vì expiry
            now < opp.expires_at
        });

        // Đánh giá lại các cơ hội còn lại
        for opp in opportunities.iter_mut() {
            // Chỉ đánh giá lại các cơ hội chưa được thực hiện
            if opp.status != MevOpportunityStatus::Detected {
                continue;
            }

            // Lấy adapter cho chain tương ứng
            if let Some(adapter) = self.evm_adapters.get(&opp.chain_id) {
                // Đánh giá lại lợi nhuận kỳ vọng
                match opp.opportunity_type {
                    MevOpportunityType::Backrun => {
                        // Simulation code for backrun profitability
                        if let Some(target_tx) = &opp.target_transaction {
                            // Kiểm tra xem giao dịch gốc đã được thực hiện chưa
                            match adapter.get_transaction_status(&target_tx.hash).await {
                                Ok(status) => {
                                    if let crate::chain_adapters::evm_adapter::TransactionStatus::Success = status {
                                        // Giao dịch đã được thực hiện, có thể không còn cơ hội
                                        opp.status = MevOpportunityStatus::Expired;
                                    }
                                },
                                Err(_) => {
                                    // Lỗi khi kiểm tra, giả định vẫn còn cơ hội
                                }
                            }
                        }
                    },
                    MevOpportunityType::Sandwich => {
                        // Cập nhật profitability dựa trên trạng thái hiện tại của mempool
                        if let Some(target_tx) = &opp.target_transaction {
                            if let Some(analyzer) = self.mempool_analyzers.get(&opp.chain_id) {
                                // Check if transaction is still in mempool
                                match analyzer.is_transaction_in_mempool(&target_tx.hash).await {
                                    Ok(in_mempool) => {
                                        if !in_mempool {
                                            // Không còn trong mempool, có thể đã được thực hiện hoặc hết hạn
                                            opp.status = MevOpportunityStatus::Expired;
                                        }
                                    },
                                    Err(_) => {
                                        // Error when checking
                                    }
                                }
                            }
                        }
                    },
                    // Handle other types...
                    _ => {}
                }
            }
        }

        // Sắp xếp lại các cơ hội theo lợi nhuận
        opportunities.sort_by(|a, b| b.expected_profit.partial_cmp(&a.expected_profit).unwrap_or(std::cmp::Ordering::Equal));
    }
    
    /// Thực thi các cơ hội MEV đã được đánh giá
    async fn execute_opportunities(&self) {
        // Lấy cấu hình hiện tại
        let config = self.config.read().await;
        if !config.auto_execute {
            return; // Không thực hiện tự động
        }

        // Kiểm tra tốc độ thực hiện
        {
            let mut executions = self.executions_this_minute.write().await;
            let now = Instant::now();
            let last_exec = *self.last_execution.read().await;
            
            // Reset counter every minute
            if now.duration_since(last_exec).as_secs() >= 60 {
                *executions = 0;
            }
            
            // Kiểm tra giới hạn thực hiện
            if *executions >= config.max_executions_per_minute {
                return; // Đã đạt giới hạn thực hiện trong phút
            }
            
            *executions += 1;
            *self.last_execution.write().await = now;
        }

        // Lấy danh sách cơ hội và chọn cơ hội tốt nhất để thực hiện
        let mut opportunities = self.opportunities.write().await;
        
        // Chỉ xem xét cơ hội đã được phát hiện và chưa thực hiện
        let executable_opportunities = opportunities.iter_mut()
            .filter(|opp| opp.status == MevOpportunityStatus::Detected)
            .filter(|opp| opp.expected_profit >= config.min_profit_threshold)
            .filter(|opp| self.evm_adapters.contains_key(&opp.chain_id))
            .take(1) // Chỉ lấy 1 cơ hội tốt nhất để thực hiện
            .collect::<Vec<_>>();
        
        for opportunity in executable_opportunities {
            let chain_id = opportunity.chain_id;
            if let Some(adapter) = self.evm_adapters.get(&chain_id) {
                // Đánh dấu là đang thực hiện
                opportunity.status = MevOpportunityStatus::Executing;
                
                // Clone opportunity để sử dụng trong task
                let opp_clone = opportunity.clone();
                let adapter_clone = adapter.clone();
                let self_clone = self.clone();
                
                // Tạo task riêng để thực hiện
                tokio::spawn(async move {
                    let result = self_clone.execute_single_opportunity(&opp_clone, &adapter_clone).await;
                    let mut opportunities = self_clone.opportunities.write().await;
                    
                    // Tìm và cập nhật trạng thái cơ hội trong danh sách
                    if let Some(opportunity) = opportunities.iter_mut().find(|o| o.id == opp_clone.id) {
                        match result {
                            Ok(profit) => {
                                opportunity.status = MevOpportunityStatus::Executed;
                                opportunity.actual_profit = Some(profit);
                                info!("Successfully executed MEV opportunity {}, profit: {} ETH", opportunity.id, profit);
                            },
                            Err(error) => {
                                opportunity.status = MevOpportunityStatus::Failed;
                                error!("Failed to execute MEV opportunity {}: {}", opportunity.id, error);
                            }
                        }
                    }
                });
            }
        }
    }
    
    /// Thực hiện một cơ hội MEV
    async fn execute_single_opportunity(&self, opportunity: &MevOpportunity, adapter: &Arc<EvmAdapter>) -> Result<f64, String> {
        // Thực hiện giao dịch MEV dựa trên loại cơ hội
        match opportunity.opportunity_type {
            MevOpportunityType::Backrun => {
                // Implement backrun execution logic
                if let Some(tx_data) = &opportunity.bundle_data {
                    // Send the backrun transaction
                    let tx_hash = adapter.send_raw_transaction(tx_data).await
                        .map_err(|e| format!("Failed to send backrun transaction: {}", e))?;
                    
                    // Wait for confirmation
                    self.wait_for_transaction_confirmation(adapter, &tx_hash).await
                        .map_err(|e| format!("Transaction failed: {}", e))?;
                    
                    // Calculate actual profit
                    let profit = self.calculate_transaction_profit(adapter, &tx_hash).await
                        .unwrap_or(0.0);
                    
                    return Ok(profit);
                }
            },
            MevOpportunityType::Sandwich => {
                // Implement sandwich execution logic
                if let (Some(first_tx), Some(second_tx)) = (&opportunity.first_transaction, &opportunity.second_transaction) {
                    // Send first transaction (front-run)
                    let first_hash = adapter.send_raw_transaction(first_tx).await
                        .map_err(|e| format!("Failed to send front-run transaction: {}", e))?;
                    
                    // Wait for target transaction to be included
                    if let Some(target_tx) = &opportunity.target_transaction {
                        self.wait_for_transaction_inclusion(adapter, &target_tx.hash).await
                            .map_err(|e| format!("Target transaction failed: {}", e))?;
                    }
                    
                    // Send second transaction (back-run)
                    let second_hash = adapter.send_raw_transaction(second_tx).await
                        .map_err(|e| format!("Failed to send back-run transaction: {}", e))?;
                    
                    // Calculate total profit
                    let profit1 = self.calculate_transaction_profit(adapter, &first_hash).await
                        .unwrap_or(0.0);
                    let profit2 = self.calculate_transaction_profit(adapter, &second_hash).await
                        .unwrap_or(0.0);
                    
                    return Ok(profit1 + profit2);
                }
            },
            _ => {
                return Err(format!("Unsupported opportunity type: {:?}", opportunity.opportunity_type));
            }
        }
        
        Err("Failed to execute opportunity: insufficient transaction data".to_string())
    }
    
    /// Phân tích cơ hội MEV nâng cao từ mempool
    async fn analyze_advanced_mempool_opportunities(&self, chain_id: u32, analyzer: &Arc<MempoolAnalyzer>) {
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return,
        };
        
        // Chạy các phân tích chuyên biệt song song
        let jit_future = self.analyze_jit_liquidity_opportunities(chain_id, analyzer, adapter);
        let cross_domain_future = self.analyze_cross_domain_opportunities(chain_id, analyzer, adapter);
        let backrunning_future = self.analyze_backrunning_opportunities(chain_id, analyzer, adapter);
        
        // Chạy tất cả phân tích đồng thời
        let (_jit_result, _cross_domain_result, _backrunning_result): ((), (), ()) = tokio::join!(
            jit_future, 
            cross_domain_future,
            backrunning_future
        );
    }
    
    /// Phân tích cơ hội JIT Liquidity
    async fn analyze_jit_liquidity_opportunities(&self, _chain_id: u32, _analyzer: &Arc<MempoolAnalyzer>, _adapter: &Arc<EvmAdapter>) {
        // Phân tích JIT Liquidity sẽ được triển khai trong tương lai
        // Đây là placeholder cho chức năng này
    }
    
    /// Phân tích cơ hội Cross-domain
    async fn analyze_cross_domain_opportunities(&self, _chain_id: u32, _analyzer: &Arc<MempoolAnalyzer>, _adapter: &Arc<EvmAdapter>) {
        // Phân tích Cross-domain sẽ được triển khai trong tương lai
        // Đây là placeholder cho chức năng này
    }
    
    /// Phân tích cơ hội Backrunning
    async fn analyze_backrunning_opportunities(&self, _chain_id: u32, _analyzer: &Arc<MempoolAnalyzer>, _adapter: &Arc<EvmAdapter>) {
        // Phân tích Backrunning sẽ được triển khai trong tương lai
        // Đây là placeholder cho chức năng này
    }
} 