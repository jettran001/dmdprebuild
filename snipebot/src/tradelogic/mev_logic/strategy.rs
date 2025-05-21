//! Strategy cho MEV Logic
//!
//! Module này triển khai các chiến lược MEV, phát hiện và tận dụng
//! các cơ hội MEV từ mempool.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::borrow::Cow;
use tokio::sync::RwLock;
use tokio::time::sleep;
use futures::future::{self, join_all};
use async_trait::async_trait;
use tracing::{debug, error, info, warn};

use crate::analys::mempool::{
    MempoolAnalyzer, MempoolTransaction, TransactionType, TransactionPriority,
    SuspiciousPattern, MempoolAlert, AlertSeverity, NewTokenInfo, create_mempool_analyzer,
    MempoolAlertType,
};
use crate::analys::token_status::{TokenStatus, TokenSafety, ContractInfo};
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::types::{ChainType, TokenPair};

use super::constants::*;
use super::types::*;
use super::traits::MevBot;
use super::analyzer;
use super::utils;

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
        
        let (pending_transactions, alerts) = tokio::join!(txs_future, alerts_future);
        
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
                MempoolAlertType::MevOpportunity => {
                    let alert_clone = alert.clone();
                    let self_clone = self.clone();
                    tokio::spawn(async move {
                        self_clone.analyze_existing_arbitrage(chain_id, alert_clone).await;
                    });
                }
            }
        }
        
        // Thêm phân tích arbitrage
        self.analyze_arbitrage_opportunities(chain_id, analyzer).await;
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
    pub async fn get_opportunities(&self) -> Cow<'_, [MevOpportunity]> {
        let opportunities = self.opportunities.read().await;
        Cow::Borrowed(&opportunities)
    }
    
    /// Lấy cấu hình hiện tại (trả về tham chiếu nếu khách hàng có thể xử lý)
    pub async fn get_opportunities_ref<'a>(&'a self) -> tokio::sync::RwLockReadGuard<'a, Vec<MevOpportunity>> {
        self.opportunities.read().await
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

    // Phần 2 của phương thức nằm trong file strategy_implementation.rs
} 