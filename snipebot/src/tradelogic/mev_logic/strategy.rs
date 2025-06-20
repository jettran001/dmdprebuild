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
use tracing::{error, info, debug, warn};
use anyhow::{Result, Context, anyhow};

use crate::analys::mempool::{
    MempoolAnalyzer, MempoolTransaction, TransactionType,
    SuspiciousPattern, MempoolAlert, AlertSeverity, NewTokenInfo, create_mempool_analyzer,
    MempoolAlertType,
};
use crate::analys::token_status::{TokenStatus, ContractInfo};
use crate::chain_adapters::evm_adapter::EvmAdapter;
use crate::chain_adapters::MevAdapterExtensions;
use crate::types::TokenPair;

use super::mev_constants::*;
use super::types::*;
use super::traits::MevBot;

/// Cấu hình cho chiến lược MEV
#[derive(Debug, Clone)]
pub struct MevConfig {
    /// Có bật chiến lược không
    pub enabled: bool,
    /// Tự động thực thi cơ hội
    pub auto_execute: bool,
    /// Số lần thực thi tối đa mỗi phút
    pub max_executions_per_minute: usize,
    /// Ngưỡng lợi nhuận tối thiểu để thực thi (ETH)
    pub min_profit_eth: f64,
    /// Ngưỡng lợi nhuận tối thiểu để thực thi
    pub min_profit_threshold: f64,
}

impl Default for MevConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_execute: false,
            max_executions_per_minute: 3,
            min_profit_eth: 0.01,
            min_profit_threshold: 0.005,
        }
    }
}

/// Loại cơ hội MEV
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MevOpportunityType {
    /// Arbitrage giữa các DEX
    Arbitrage,
    /// Sandwich attack
    Sandwich,
    /// Front-running
    FrontRun,
    /// Back-running
    BackRun,
    /// Token mới với thanh khoản cao
    NewToken,
    /// Thêm thanh khoản mới
    NewLiquidity,
    /// Rút thanh khoản
    LiquidityRemoval,
    /// Giao dịch whale
    WhaleTransaction,
    /// JIT Liquidity
    JitLiquidity,
    /// Cross-domain MEV
    CrossDomain,
}

/// Phương thức thực thi MEV
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MevExecutionMethod {
    /// Sử dụng flash loan
    FlashLoan,
    /// Giao dịch tiêu chuẩn
    Standard,
    /// Multi-swap
    MultiSwap,
    /// Gọi hợp đồng tùy chỉnh
    CustomContract,
    /// Sử dụng MEV bundle
    MevBundle,
    /// Sử dụng RPC riêng tư
    PrivateRPC,
    /// Sử dụng Builder API
    BuilderAPI,
}

/// Trạng thái cơ hội MEV
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MevOpportunityStatus {
    /// Đã phát hiện, chưa thực thi
    Detected,
    /// Đang thực thi
    Executing,
    /// Đã thực thi thành công
    Executed,
    /// Thực thi thất bại
    Failed,
    /// Đã hết hạn
    Expired,
}

/// Cơ hội MEV
#[derive(Debug, Clone)]
pub struct MevOpportunity {
    /// ID duy nhất của cơ hội
    pub id: String,
    /// Loại cơ hội
    pub opportunity_type: MevOpportunityType,
    /// Chain ID
    pub chain_id: u32,
    /// Lợi nhuận dự kiến (ETH)
    pub expected_profit: f64,
    /// Lợi nhuận ròng ước tính (USD)
    pub estimated_net_profit_usd: f64,
    /// Điểm rủi ro (1-10)
    pub risk_score: u8,
    /// Phương thức thực thi
    pub execution_method: MevExecutionMethod,
    /// Trạng thái hiện tại
    pub status: MevOpportunityStatus,
    /// Thời điểm phát hiện (Unix timestamp)
    pub timestamp: u64,
    /// Thời điểm hết hạn (Unix timestamp)
    pub expires_at: u64,
    /// Lợi nhuận thực tế sau khi thực thi
    pub actual_profit: Option<f64>,
    /// Giao dịch mục tiêu
    pub target_transaction: Option<String>,
    /// Các giao dịch liên quan
    pub related_transactions: Vec<String>,
    /// Cặp token liên quan
    pub token_pair: Option<TokenPair>,
    /// Tham số cụ thể cho cơ hội
    pub specific_params: HashMap<String, String>,
    /// Dữ liệu bundle (cho MEV-Bundle)
    pub bundle_data: Option<String>,
    /// Chi phí gas ước tính (USD)
    pub estimated_gas_cost_usd: f64,
    /// Hash của giao dịch sau khi thực thi
    pub transaction_hash: Option<String>,
}

/// Cặp token
#[derive(Debug, Clone)]
pub struct TokenPair {
    /// Token đầu tiên
    pub token0: String,
    /// Token thứ hai
    pub token1: String,
    /// Địa chỉ token base
    pub base_token_address: String,
    /// Địa chỉ token
    pub token_address: String,
    /// Token base
    pub base_token: String,
    /// Token quote
    pub quote_token: String,
    /// Chain ID
    pub chain_id: u32,
}

/// Chiến lược phát hiện và tận dụng MEV
///
/// MEVStrategy quản lý phát hiện, phân tích và thực thi các cơ hội MEV.
/// Nó hoạt động bằng cách phân tích mempool, theo dõi các giao dịch đáng ngờ,
/// và thực thi các cơ hội khi chúng đáp ứng các tiêu chí cấu hình.
#[derive(Clone)]
pub struct MevStrategy {
    /// Mempool analyzer cho mỗi chain được theo dõi
    mempool_analyzers: HashMap<u32, Arc<MempoolAnalyzer>>,
    /// Các EVM adapter tương ứng
    evm_adapters: HashMap<u32, Arc<EvmAdapter>>,
    /// Cơ hội MEV đã phát hiện
    opportunities: Arc<RwLock<Vec<MevOpportunity>>>,
    /// Cấu hình MEV
    config: Arc<RwLock<MevConfig>>,
    /// Thời gian lần cuối thực thi
    last_execution: Arc<RwLock<Instant>>,
    /// Số lần thực thi trong phút hiện tại
    executions_this_minute: Arc<RwLock<u32>>,
    /// Đang chạy hay không
    running: Arc<RwLock<bool>>,
    /// Cache cho thông tin contract
    contract_info_cache: Arc<RwLock<HashMap<String, ContractInfo>>>,
}

impl MevStrategy {
    /// Tạo mới MEV strategy
    pub fn new() -> Self {
        Self {
            mempool_analyzers: HashMap::<u32, Arc<MempoolAnalyzer>>::new(),
            evm_adapters: HashMap::<u32, Arc<EvmAdapter>>::new(),
            opportunities: Arc::new(RwLock::new(Vec::new())),
            config: Arc::new(RwLock::new(MevConfig::default())),
            last_execution: Arc::new(RwLock::new(Instant::now())),
            executions_this_minute: Arc::new(RwLock::new(0)),
            running: Arc::new(RwLock::new(false)),
            contract_info_cache: Arc::new(RwLock::new(HashMap::<String, ContractInfo>::new())),
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
        
        // Clone các thông tin cần thiết cho task
        let self_arc = Arc::new(self.clone());
        
        // Khởi động task theo dõi
        tokio::spawn(async move {
            self_arc.monitor_loop().await;
        });
    }
    
    /// Dừng theo dõi MEV
    pub async fn stop(&self) {
        let mut running = self.running.write().await;
        *running = false;
    }
    
    /// Khoảng thời gian giữa các lần quét (ms)
    fn scan_interval_ms(&self) -> u64 {
        500 // Mặc định 500ms
    }
    
    /// Vòng lặp theo dõi chính
    async fn monitor_loop(&self) {
        while *self.running.read().await {
            // Thực hiện tác vụ song song thay vì tuần tự
            let scan_future = self.scan_mev_opportunities();
            let evaluate_future = self.evaluate_opportunities();
            
            // Chạy đồng thời các tác vụ không phụ thuộc nhau
            let (scan_result, evaluate_result) = tokio::join!(scan_future, evaluate_future);
            
            // Xử lý lỗi nếu có
            if let Err(e) = scan_result {
                error!("Lỗi khi quét cơ hội MEV: {}", e);
            }
            
            if let Err(e) = evaluate_result {
                error!("Lỗi khi đánh giá cơ hội MEV: {}", e);
            }
            
            // Xử lý thực thi cơ hội
            if let Err(e) = self.execute_opportunities().await {
                error!("Lỗi khi thực thi cơ hội MEV: {}", e);
            }
            
            // Chờ một khoảng thời gian trước khi quét lại
            sleep(Duration::from_millis(self.scan_interval_ms())).await;
        }
    }
    
    /// Quét cơ hội MEV từ mempool
    async fn scan_mev_opportunities(&self) -> Result<()> {
        // Chạy song song phân tích cho tất cả chains
        let futures = self.mempool_analyzers.iter().map(|(chain_id, analyzer)| {
            self.scan_chain_opportunities(*chain_id, analyzer)
        });
        
        // Chờ tất cả các futures hoàn thành
        let results = join_all(futures).await;
        
        // Kiểm tra và xử lý lỗi
        for result in results {
            if let Err(e) = result {
                error!("Lỗi khi quét cơ hội MEV: {}", e);
            }
        }
        
        Ok(())
    }
    
    /// Quét cơ hội MEV cho một chain cụ thể
    async fn scan_chain_opportunities(&self, chain_id: u32, analyzer: &Arc<MempoolAnalyzer>) -> Result<()> {
        // Lấy giao dịch đồng thời với lấy cảnh báo
        let txs_future = analyzer.get_pending_transactions(100);
        let alerts_future = analyzer.get_alerts(Some(AlertSeverity::Medium));
        
        // Chỉ định kiểu dữ liệu để tránh lỗi E0282: type annotations needed
        let (pending_transactions, alerts): (Vec<MempoolTransaction>, Vec<MempoolAlert>) = tokio::join!(txs_future, alerts_future);
        
        // Phân tích giao dịch
        self.analyze_pending_transactions(chain_id, &pending_transactions).await?;
        
        // Xử lý cảnh báo từ mempool analyzer
        for alert in alerts {
            match alert.alert_type {
                MempoolAlertType::NewToken => {
                    if let Some(token_address) = &alert.target_transaction {
                        // Lấy thông tin token mới
                        let adapter = match self.evm_adapters.get(&chain_id) {
                            Some(adapter) => adapter,
                            None => continue,
                        };
                        
                        if let Ok(token_info) = adapter.get_token_info(token_address).await {
                            // Tạo thông tin token mới
                            let new_token = NewTokenInfo {
                                token_address: token_address.clone(),
                                chain_id,
                                creator_address: token_info.contract_creator.unwrap_or_default(),
                                creation_tx: String::new(),
                                detected_at: alert.detected_at,
                                liquidity_added: false,
                                liquidity_tx: token_info.liquidity_eth.map(|_| String::new()),
                                liquidity_value: None,
                                dex_type: None,
                            };
                            
                            // Xử lý token mới
                            self.process_new_token(chain_id, new_token, alert).await?;
                        }
                    }
                },
                MempoolAlertType::SuspiciousActivity => {
                    if let Some(pattern) = &alert.suspicious_pattern {
                        match pattern {
                            SuspiciousPattern::LiquidityRemoval => {
                                // Xử lý cảnh báo rút thanh khoản
                                self.process_liquidity_removal(chain_id, alert).await?;
                            },
                            SuspiciousPattern::FrontRunning => {
                                // Xử lý cảnh báo front-running
                                self.process_front_running_opportunity(chain_id, alert).await?;
                            },
                            SuspiciousPattern::Sandwich => {
                                // Xử lý cảnh báo sandwich
                                self.process_sandwich_opportunity(chain_id, alert).await?;
                            },
                            SuspiciousPattern::WhaleMovement => {
                                // Xử lý cảnh báo whale movement
                                self.process_whale_transaction(chain_id, alert).await?;
                            },
                            _ => {
                                // Xử lý các pattern khác
                                debug!("Phát hiện hoạt động đáng ngờ khác: {:?}", pattern);
                            }
                        }
                    }
                },
                _ => {}
            }
        }
        
        // Phân tích cơ hội MEV nâng cao
        self.analyze_advanced_mempool_opportunities(chain_id, analyzer).await?;
        
        Ok(())
    }
    
    /// Phân tích trực tiếp các giao dịch đang chờ trong mempool
    async fn analyze_pending_transactions(&self, chain_id: u32, transactions: &[MempoolTransaction]) -> Result<()> {
        let config = self.config.read().await;
        if !config.enabled {
            return Ok(());
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
                TransactionType::TokenApproval => token_approvals.push(tx),
                TransactionType::Transfer => {
                    if tx.value_usd > 10000.0 { // Ngưỡng $10,000
                        high_value_transfers.push(tx);
                    }
                }
            }
        }
        
        // Phân tích các giao dịch swap - chỉ rõ kiểu dữ liệu cho Vec
        let swap_refs: Vec<&MempoolTransaction> = swaps.iter().collect::<Vec<&MempoolTransaction>>();
        self.detect_sandwich_targets(chain_id, &swap_refs).await?;
        self.detect_potential_arbitrage_paths(chain_id, &swap_refs).await?;
        
        // Phân tích các sự kiện thanh khoản
        self.analyze_liquidity_events(chain_id, &liquidity_events).await?;
        
        // Phân tích mối tương quan giữa approve và swap
        self.correlate_approvals_and_swaps(chain_id, &token_approvals, &swaps).await?;
        
        // Phân tích các giao dịch có giá trị cao
        for tx in high_value_transfers {
            debug!("Phát hiện giao dịch giá trị cao: {} ETH từ {}", tx.value_eth, tx.from_address);
        }
        
        Ok(())
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
        let (source_code, is_verified) = source_code.ok_or_else(|| anyhow!("Không thể lấy source code")).unwrap_or_else(|_| (None, false));
        let bytecode = bytecode.ok_or_else(|| anyhow!("Không thể lấy bytecode")).unwrap_or_else(|_| None);
        
        // Chỉ lấy ABI nếu contract đã được xác minh
        let abi = if is_verified { 
            adapter.get_contract_abi(token_address).await.with_context(|| format!("Không thể lấy ABI cho contract {}", token_address)).unwrap_or(None) 
        } else { 
            None 
        };
        
        // Lấy thông tin chủ sở hữu
        let owner_address = adapter.get_contract_owner(token_address).await.with_context(|| format!("Không thể lấy thông tin chủ sở hữu cho contract {}", token_address)).unwrap_or(None);
        
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
    
    /// Thực thi các cơ hội MEV đã được đánh giá
    async fn execute_opportunities(&self) -> Result<()> {
        // Lấy cấu hình hiện tại
        let config = self.config.read().await;
        if !config.auto_execute {
            return Ok(()); // Không thực hiện tự động
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
            if *executions >= config.max_executions_per_minute as usize {
                debug!("Đã đạt giới hạn thực hiện mỗi phút: {}", config.max_executions_per_minute);
                return Ok(());
            }
        }

        // Lấy danh sách cơ hội hiện tại
        let mut opportunities = self.opportunities.write().await;
        
        // Lọc các cơ hội đã sẵn sàng thực hiện
        let ready_opportunities: Vec<&mut MevOpportunity> = opportunities.iter_mut()
            .filter(|opp| {
                opp.status == MevOpportunityStatus::Detected && 
                opp.estimated_net_profit_usd >= config.min_profit_threshold
            })
            .collect();
        
        // Thực hiện các cơ hội theo thứ tự lợi nhuận giảm dần
        for opp in ready_opportunities {
            // Chỉ thực hiện nếu cơ hội vẫn còn hợp lệ
            if opp.expires_at < SystemTime::now().duration_since(UNIX_EPOCH).context("Lỗi khi lấy thời gian hiện tại")?.as_secs() {
                opp.status = MevOpportunityStatus::Expired;
                continue;
            }
            
            // Lấy adapter cho chain này
            let adapter = match self.evm_adapters.get(&opp.chain_id) {
                Some(adapter) => adapter,
                None => {
                    error!("Không tìm thấy adapter cho chain ID {}", opp.chain_id);
                    continue;
                }
            };
            
            // Thực hiện cơ hội dựa trên loại
            match opp.opportunity_type {
                MevOpportunityType::Arbitrage => {
                    info!("Thực hiện arbitrage: {}", opp.id);
                    // Thực hiện arbitrage
                    match self.execute_arbitrage(adapter, opp).await {
                        Ok(tx_hash) => {
                            info!("Đã thực hiện arbitrage: {}, tx hash: {}", opp.id, tx_hash);
                            opp.status = MevOpportunityStatus::Executed;
                            opp.transaction_hash = Some(tx_hash);
                        },
                        Err(e) => {
                            error!("Lỗi khi thực hiện arbitrage {}: {}", opp.id, e);
                            opp.status = MevOpportunityStatus::Failed;
                        }
                    }
                },
                MevOpportunityType::Sandwich => {
                    info!("Thực hiện sandwich: {}", opp.id);
                    // Thực hiện sandwich
                    match self.execute_sandwich(adapter, opp).await {
                        Ok(tx_hash) => {
                            info!("Đã thực hiện sandwich: {}, tx hash: {}", opp.id, tx_hash);
                            opp.status = MevOpportunityStatus::Executed;
                            opp.transaction_hash = Some(tx_hash);
                        },
                        Err(e) => {
                            error!("Lỗi khi thực hiện sandwich {}: {}", opp.id, e);
                            opp.status = MevOpportunityStatus::Failed;
                        }
                    }
                },
                MevOpportunityType::FrontRun => {
                    info!("Thực hiện front-running: {}", opp.id);
                    // Thực hiện front-running
                    match self.execute_frontrun(adapter, opp).await {
                        Ok(tx_hash) => {
                            info!("Đã thực hiện front-running: {}, tx hash: {}", opp.id, tx_hash);
                            opp.status = MevOpportunityStatus::Executed;
                            opp.transaction_hash = Some(tx_hash);
                        },
                        Err(e) => {
                            error!("Lỗi khi thực hiện front-running {}: {}", opp.id, e);
                            opp.status = MevOpportunityStatus::Failed;
                        }
                    }
                },
                MevOpportunityType::BackRun => {
                    info!("Thực hiện back-running: {}", opp.id);
                    // Thực hiện back-running
                    match self.execute_backrun(adapter, opp).await {
                        Ok(tx_hash) => {
                            info!("Đã thực hiện back-running: {}, tx hash: {}", opp.id, tx_hash);
                            opp.status = MevOpportunityStatus::Executed;
                            opp.transaction_hash = Some(tx_hash);
                        },
                        Err(e) => {
                            error!("Lỗi khi thực hiện back-running {}: {}", opp.id, e);
                            opp.status = MevOpportunityStatus::Failed;
                        }
                    }
                },
                MevOpportunityType::LiquidityRemoval => {
                    // Không thực hiện tự động với cơ hội rút thanh khoản
                    debug!("Bỏ qua cơ hội rút thanh khoản: {}", opp.id);
                },
                MevOpportunityType::NewLiquidity => {
                    // Thực hiện snipe token mới
                    info!("Thực hiện snipe token mới: {}", opp.id);
                    match self.execute_new_token_snipe(adapter, opp).await {
                        Ok(tx_hash) => {
                            info!("Đã snipe token mới: {}, tx hash: {}", opp.id, tx_hash);
                            opp.status = MevOpportunityStatus::Executed;
                            opp.transaction_hash = Some(tx_hash);
                        },
                        Err(e) => {
                            error!("Lỗi khi snipe token mới {}: {}", opp.id, e);
                            opp.status = MevOpportunityStatus::Failed;
                        }
                    }
                },
                MevOpportunityType::JitLiquidity => {
                    // Thực hiện JIT liquidity
                    info!("Thực hiện JIT liquidity: {}", opp.id);
                    match self.execute_jit_liquidity(adapter, opp).await {
                        Ok(tx_hash) => {
                            info!("Đã thực hiện JIT liquidity: {}, tx hash: {}", opp.id, tx_hash);
                            opp.status = MevOpportunityStatus::Executed;
                            opp.transaction_hash = Some(tx_hash);
                        },
                        Err(e) => {
                            error!("Lỗi khi thực hiện JIT liquidity {}: {}", opp.id, e);
                            opp.status = MevOpportunityStatus::Failed;
                        }
                    }
                },
                MevOpportunityType::CrossDomain => {
                    // Thực hiện cross-domain MEV
                    info!("Thực hiện cross-domain MEV: {}", opp.id);
                    match self.execute_cross_domain(adapter, opp).await {
                        Ok(tx_hash) => {
                            info!("Đã thực hiện cross-domain MEV: {}, tx hash: {}", opp.id, tx_hash);
                            opp.status = MevOpportunityStatus::Executed;
                            opp.transaction_hash = Some(tx_hash);
                        },
                        Err(e) => {
                            error!("Lỗi khi thực hiện cross-domain MEV {}: {}", opp.id, e);
                            opp.status = MevOpportunityStatus::Failed;
                        }
                    }
                },
            }
            
            // Cập nhật counter và timestamp
            {
                let mut executions = self.executions_this_minute.write().await;
                *executions += 1;
                *self.last_execution.write().await = Instant::now();
            }
            
            // Chỉ thực hiện một cơ hội mỗi lần
            break;
        }
        
        Ok(())
    }
    
    /// Thực thi flash loan
    async fn execute_flash_loan(&self, opportunity: &MevOpportunity, adapter: &Arc<EvmAdapter>) -> anyhow::Result<f64> {
        // Validate flash loan parameters
        let params = &opportunity.specific_params;
        if !params.contains_key("loan_amount") || !params.contains_key("loan_token") {
            return Err(anyhow::anyhow!("Missing flash loan parameters"));
        }

        let loan_token = params.get("loan_token")
            .ok_or_else(|| anyhow::anyhow!("Missing loan token"))?;
        
        let loan_amount_str = params.get("loan_amount")
            .ok_or_else(|| anyhow::anyhow!("Missing loan amount"))?;
            
        let loan_amount = loan_amount_str.parse::<f64>()
            .with_context(|| format!("Invalid loan amount: {}", loan_amount_str))?;

        // Execute flash loan
        match adapter.execute_flash_loan(
            loan_token,
            loan_amount,
            &opportunity.related_transactions
        ).await {
            Ok(profit) => {
                // Update execution stats
                let mut executions = self.executions_this_minute.write().await;
                *executions += 1;
                Ok(profit)
            },
            Err(e) => Err(anyhow::anyhow!("Flash loan execution failed: {}", e))
        }
    }
    
    /// Thực thi giao dịch thông thường
    async fn execute_standard_trade(&self, opportunity: &MevOpportunity, adapter: &Arc<EvmAdapter>) -> anyhow::Result<f64> {
        // Validate standard trade parameters
        if opportunity.related_transactions.is_empty() {
            return Err(anyhow::anyhow!("No transactions to execute"));
        }

        // Execute standard trade
        match adapter.execute_transaction(
            &opportunity.related_transactions[0],
            opportunity.estimated_gas_cost_usd
        ).await {
            Ok(profit) => {
                // Update execution stats
                let mut executions = self.executions_this_minute.write().await;
                *executions += 1;
                Ok(profit)
            },
            Err(e) => Err(anyhow::anyhow!("Standard trade execution failed: {}", e))
        }
    }
    
    /// Thực thi multi swap
    async fn execute_multi_swap(&self, opportunity: &MevOpportunity, adapter: &Arc<EvmAdapter>) -> anyhow::Result<f64> {
        // Validate multi swap parameters
        if opportunity.related_transactions.len() < 2 {
            return Err(anyhow::anyhow!("Not enough transactions for multi swap"));
        }

        // Execute multi swap
        match adapter.execute_multi_swap(
            &opportunity.related_transactions,
            opportunity.estimated_gas_cost_usd
        ).await {
            Ok(profit) => {
                // Update execution stats
                let mut executions = self.executions_this_minute.write().await;
                *executions += 1;
                Ok(profit)
            },
            Err(e) => Err(anyhow::anyhow!("Multi swap execution failed: {}", e))
        }
    }
    
    /// Thực thi custom contract
    async fn execute_custom_contract(&self, opportunity: &MevOpportunity, adapter: &Arc<EvmAdapter>) -> anyhow::Result<f64> {
        // Validate custom contract parameters
        if !opportunity.specific_params.contains_key("contract_address") {
            return Err(anyhow::anyhow!("Missing contract address"));
        }

        let contract_address = opportunity.specific_params.get("contract_address")
            .ok_or_else(|| anyhow::anyhow!("Contract address not found"))?;

        // Execute custom contract
        match adapter.execute_custom_contract(
            contract_address,
            &opportunity.related_transactions,
            opportunity.estimated_gas_cost_usd
        ).await {
            Ok(profit) => {
                // Update execution stats
                let mut executions = self.executions_this_minute.write().await;
                *executions += 1;
                Ok(profit)
            },
            Err(e) => Err(anyhow::anyhow!("Custom contract execution failed: {}", e))
        }
    }
    
    /// Thực thi MEV bundle
    async fn execute_mev_bundle(&self, opportunity: &MevOpportunity, adapter: &Arc<EvmAdapter>) -> anyhow::Result<f64> {
        // Validate MEV bundle parameters
        let bundle_data = opportunity.bundle_data
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing bundle data"))?;

        // Execute MEV bundle
        match adapter.execute_mev_bundle(
            bundle_data,
            opportunity.estimated_gas_cost_usd
        ).await {
            Ok(profit) => {
                // Update execution stats
                let mut executions = self.executions_this_minute.write().await;
                *executions += 1;
                Ok(profit)
            },
            Err(e) => Err(anyhow::anyhow!("MEV bundle execution failed: {}", e))
        }
    }
    
    /// Thực thi private RPC
    async fn execute_private_rpc(&self, opportunity: &MevOpportunity, adapter: &Arc<EvmAdapter>) -> anyhow::Result<f64> {
        // Validate private RPC parameters
        if opportunity.related_transactions.is_empty() {
            return Err(anyhow::anyhow!("No transactions to execute"));
        }

        // Execute private RPC
        match adapter.execute_private_rpc(
            &opportunity.related_transactions[0],
            opportunity.estimated_gas_cost_usd
        ).await {
            Ok(profit) => {
                // Update execution stats
                let mut executions = self.executions_this_minute.write().await;
                *executions += 1;
                Ok(profit)
            },
            Err(e) => Err(anyhow::anyhow!("Private RPC execution failed: {}", e))
        }
    }
    
    /// Thực thi builder API
    async fn execute_builder_api(&self, opportunity: &MevOpportunity, adapter: &Arc<EvmAdapter>) -> anyhow::Result<f64> {
        // Validate builder API parameters
        if opportunity.related_transactions.is_empty() {
            return Err(anyhow::anyhow!("No transactions to execute"));
        }

        // Execute builder API
        match adapter.execute_builder_api(
            &opportunity.related_transactions[0],
            opportunity.estimated_gas_cost_usd
        ).await {
            Ok(profit) => {
                // Update execution stats
                let mut executions = self.executions_this_minute.write().await;
                *executions += 1;
                Ok(profit)
            },
            Err(e) => Err(anyhow::anyhow!("Builder API execution failed: {}", e))
        }
    }
    
    /// Phân tích cơ hội MEV nâng cao từ mempool
    async fn analyze_advanced_mempool_opportunities(&self, chain_id: u32, analyzer: &Arc<MempoolAnalyzer>) -> Result<()> {
        // Lấy adapter cho chain
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => return Ok(()),
        };
        
        // Chạy các phân tích chuyên biệt song song
        let jit_future = self.analyze_jit_liquidity_opportunities(chain_id, analyzer, adapter);
        let cross_domain_future = self.analyze_cross_domain_opportunities(chain_id, analyzer, adapter);
        let backrunning_future = self.analyze_backrunning_opportunities(chain_id, analyzer, adapter);
        
        // Chạy tất cả phân tích đồng thời
        let (jit_result, cross_domain_result, backrunning_result) = 
            tokio::join!(jit_future, cross_domain_future, backrunning_future);
        
        // Xử lý lỗi nếu có
        if let Err(e) = jit_result {
            error!("Lỗi khi phân tích cơ hội JIT liquidity: {}", e);
        }
        
        if let Err(e) = cross_domain_result {
            error!("Lỗi khi phân tích cơ hội cross-domain: {}", e);
        }
        
        if let Err(e) = backrunning_result {
            error!("Lỗi khi phân tích cơ hội backrunning: {}", e);
        }
        
        Ok(())
    }
    
    /// Phân tích cơ hội JIT Liquidity
    /// Phân tích cơ hội JIT Liquidity
    async fn analyze_jit_liquidity_opportunities(
        &self, 
        _chain_id: u32, 
        _analyzer: &Arc<MempoolAnalyzer>, 
        _adapter: &Arc<EvmAdapter>
    ) -> Result<()> {
        // Phân tích JIT Liquidity sẽ được triển khai trong tương lai
        // Đây là placeholder cho chức năng này
        Ok(())
    }
    
    /// Phân tích cơ hội Cross-domain
    async fn analyze_cross_domain_opportunities(
        &self, 
        _chain_id: u32, 
        _analyzer: &Arc<MempoolAnalyzer>, 
        _adapter: &Arc<EvmAdapter>
    ) -> Result<()> {
        // Phân tích Cross-domain sẽ được triển khai trong tương lai
        // Đây là placeholder cho chức năng này
        Ok(())
    }
    
    /// Phân tích cơ hội Backrunning
    async fn analyze_backrunning_opportunities(
        &self, 
        _chain_id: u32, 
        _analyzer: &Arc<MempoolAnalyzer>, 
        _adapter: &Arc<EvmAdapter>
    ) -> Result<()> {
        // Phân tích Backrunning sẽ được triển khai trong tương lai
        // Đây là placeholder cho chức năng này
        Ok(())
    }

    /// Xử lý token mới được phát hiện
    async fn process_new_token(&self, chain_id: u32, token: NewTokenInfo, alert: MempoolAlert) -> Result<()> {
        let config = self.config.read().await;
        if !config.enabled {
            return Ok(());
        }

        // Kiểm tra token có đủ điều kiện không
        if token.liquidity_tx < config.min_profit_eth {
            return Ok(());
        }

        // Kiểm tra an toàn của token
        let token_address = &token.token_address;
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => {
                error!("Không tìm thấy adapter cho chain ID {}", chain_id);
                return Ok(());
            }
        };

        // Lấy giá ETH hiện tại hoặc mặc định nếu không có
        let eth_price = match &token.eth_price_usd {
            Some(price) => *price,
            None => {
                // Nếu không có giá ETH, sử dụng giá trị mặc định
                warn!("Missing ETH price for token {}, using default of 1800 USD", token_address);
                1800.0
            }
        };

        // Tạo cơ hội MEV
        let opportunity = MevOpportunity {
            id: format!("new_token_{}", token.token_address),
            opportunity_type: MevOpportunityType::NewToken,
            chain_id,
            expected_profit: token.liquidity_tx,
            estimated_net_profit_usd: token.liquidity_tx * eth_price,
            risk_score: 3,
            execution_method: MevExecutionMethod::MevBundle,
            status: MevOpportunityStatus::Detected,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("Lỗi khi lấy thời gian hiện tại")?
                .as_secs(),
            expires_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("Lỗi khi tính thời gian hết hạn")?
                .as_secs() + 300, // 5 phút
            actual_profit: None,
            target_transaction: None,
            related_transactions: Vec::new(),
            token_pair: Some(TokenPair {
                token0: token.token_address.clone(),
                token1: "WETH".to_string(),
                base_token_address: "WETH".to_string(),
                token_address: token.token_address.clone(),
                base_token: "WETH".to_string(),
                quote_token: token.token_address.clone(),
                chain_id,
            }),
            specific_params: HashMap::<String, String>::new(),
            bundle_data: None,
            estimated_gas_cost_usd: 0.0,
            transaction_hash: None,
        };

        // Thêm vào danh sách cơ hội
        let mut opportunities = self.opportunities.write().await;
        opportunities.push(opportunity);

        Ok(())
    }

    /// Xử lý liquidity mới được thêm vào
    async fn process_new_liquidity(&self, chain_id: u32, alert: MempoolAlert) -> Result<()> {
        let config = self.config.read().await;
        if !config.enabled {
            return Ok(());
        }

        // Tạo cơ hội MEV
        let opportunity = MevOpportunity {
            id: format!("liquidity_added_{}", alert.id.as_ref().map_or("unknown", |id| id)),
            opportunity_type: MevOpportunityType::NewLiquidity,
            chain_id,
            expected_profit: 0.0,
            estimated_net_profit_usd: 0.0,
            risk_score: 2,
            execution_method: MevExecutionMethod::Standard,
            status: MevOpportunityStatus::Detected,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("Lỗi khi lấy thời gian hiện tại")?
                .as_secs(),
            expires_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("Lỗi khi lấy thời gian hiện tại")?
                .as_secs() + 300, // Hết hạn sau 5 phút
            actual_profit: None,
            target_transaction: alert.target_transaction.clone(),
            related_transactions: alert.related_transactions.clone().unwrap_or_default(),
            token_pair: None,
            specific_params: HashMap::<String, String>::new(),
            bundle_data: None,
            estimated_gas_cost_usd: 0.0,
            transaction_hash: None,
        };

        // Thêm cơ hội vào danh sách
        {
            let mut opportunities = self.opportunities.write().await;
            opportunities.push(opportunity);
        }

        Ok(())
    }

    /// Xử lý liquidity bị rút ra
    async fn process_liquidity_removal(&self, chain_id: u32, alert: MempoolAlert) -> Result<()> {
        let config = self.config.read().await;
        if !config.enabled {
            return Ok(());
        }

        // Tạo cơ hội MEV
        let opportunity = MevOpportunity {
            id: format!("liquidity_removed_{}", alert.id),
            opportunity_type: MevOpportunityType::LiquidityRemoval,
            chain_id,
            expected_profit: 0.0,
            estimated_net_profit_usd: 0.0,
            risk_score: 4,
            execution_method: MevExecutionMethod::Standard,
            status: MevOpportunityStatus::Detected,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("Lỗi khi lấy thời gian hiện tại")?
                .as_secs(),
            expires_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("Lỗi khi tính thời gian hết hạn")?
                .as_secs() + 300, // 5 phút
            actual_profit: None,
            target_transaction: None,
            related_transactions: Vec::new(),
            token_pair: None,
            specific_params: HashMap::<String, String>::new(),
            bundle_data: None,
            estimated_gas_cost_usd: 0.0,
            transaction_hash: None,
        };

        // Thêm vào danh sách cơ hội
        let mut opportunities = self.opportunities.write().await;
        opportunities.push(opportunity);

        Ok(())
    }

    /// Xử lý cơ hội front running
    async fn process_front_running_opportunity(&self, chain_id: u32, alert: MempoolAlert) -> Result<()> {
        let config = self.config.read().await;
        if !config.enabled {
            return Ok(());
        }

        // Tạo cơ hội MEV
        let opportunity = MevOpportunity {
            id: format!("front_running_{}", alert.id.as_ref().map_or("unknown", |id| id)),
            opportunity_type: MevOpportunityType::FrontRun,
            chain_id,
            expected_profit: 0.0,
            estimated_net_profit_usd: 0.0,
            risk_score: 5,
            execution_method: MevExecutionMethod::Standard,
            status: MevOpportunityStatus::Detected,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("Lỗi khi lấy thời gian hiện tại")?
                .as_secs(),
            expires_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("Lỗi khi tính thời gian hết hạn")?
                .as_secs() + 300, // 5 phút
            actual_profit: None,
            target_transaction: alert.target_transaction.clone(),
            related_transactions: alert.related_transactions.clone().unwrap_or_default(),
            token_pair: None,
            specific_params: HashMap::<String, String>::new(),
            bundle_data: None,
            estimated_gas_cost_usd: 0.0,
            transaction_hash: None,
        };

        // Thêm vào danh sách cơ hội
        let mut opportunities = self.opportunities.write().await;
        opportunities.push(opportunity);

        Ok(())
    }

    /// Xử lý cơ hội sandwich
    async fn process_sandwich_opportunity(&self, chain_id: u32, alert: MempoolAlert) -> Result<()> {
        let config = self.config.read().await;
        if !config.enabled {
            return Ok(());
        }

        // Tạo cơ hội MEV
        let opportunity = MevOpportunity {
            id: format!("sandwich_{}", alert.id.as_ref().map_or("unknown", |id| id)),
            opportunity_type: MevOpportunityType::Sandwich,
            chain_id,
            expected_profit: 0.0,
            estimated_net_profit_usd: 0.0,
            risk_score: 6,
            execution_method: MevExecutionMethod::MevBundle,
            status: MevOpportunityStatus::Detected,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("Lỗi khi lấy thời gian hiện tại")?
                .as_secs(),
            expires_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("Lỗi khi lấy thời gian hiện tại")?
                .as_secs() + 60, // Hết hạn sau 60 giây
            actual_profit: None,
            target_transaction: alert.target_transaction.clone(),
            related_transactions: alert.related_transactions.clone().unwrap_or_default(),
            token_pair: None,
            specific_params: HashMap::<String, String>::new(),
            bundle_data: None,
            estimated_gas_cost_usd: 0.0,
            transaction_hash: None,
        };

        // Thêm cơ hội vào danh sách
        {
            let mut opportunities = self.opportunities.write().await;
            opportunities.push(opportunity);
        }

        Ok(())
    }
    
    /// Xử lý giao dịch whale
    async fn process_whale_transaction(&self, chain_id: u32, alert: MempoolAlert) -> Result<()> {
        let config = self.config.read().await;
        if !config.enabled {
            return Ok(());
        }

        // Tạo cơ hội MEV
        let opportunity = MevOpportunity {
            id: format!("whale_{}", alert.id.as_ref().map_or("unknown", |id| id)),
            opportunity_type: MevOpportunityType::BackRun,
            chain_id,
            expected_profit: 0.0,
            estimated_net_profit_usd: 0.0,
            risk_score: 3,
            execution_method: MevExecutionMethod::Standard,
            status: MevOpportunityStatus::Detected,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("Lỗi khi lấy thời gian hiện tại")?
                .as_secs(),
            expires_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("Lỗi khi lấy thời gian hiện tại")?
                .as_secs() + 120, // Hết hạn sau 120 giây
            actual_profit: None,
            target_transaction: alert.target_transaction.clone(),
            related_transactions: alert.related_transactions.clone().unwrap_or_default(),
            token_pair: None,
            specific_params: HashMap::<String, String>::new(),
            bundle_data: None,
            estimated_gas_cost_usd: 0.0,
            transaction_hash: None,
        };

        // Thêm cơ hội vào danh sách
        {
            let mut opportunities = self.opportunities.write().await;
            opportunities.push(opportunity);
        }

        Ok(())
    }

    /// Phát hiện các đường arbitrage tiềm năng
    async fn detect_potential_arbitrage_paths(&self, chain_id: u32, swaps: &[&MempoolTransaction]) -> Result<()> {
        // Lấy adapter cho chain này
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => {
                error!("Không tìm thấy adapter cho chain ID {}", chain_id);
                return Ok(());
            }
        };

        // Nhóm các swap theo cặp token
        let mut token_pair_swaps: HashMap<String, Vec<&MempoolTransaction>> = HashMap::new();
        
        for &tx in swaps {
            if tx.transaction_type == TransactionType::Swap {
                // Xây dựng key cho token pair (token0-token1)
                if let (Some(from_token), Some(to_token)) = (&tx.from_token, &tx.to_token) {
                    let pair_key = format!("{}-{}", from_token.address, to_token.address);
                    token_pair_swaps.entry(pair_key).or_insert_with(Vec::new).push(tx);
                    
                    // Thêm cả chiều ngược lại
                    let reverse_key = format!("{}-{}", to_token.address, from_token.address);
                    token_pair_swaps.entry(reverse_key).or_insert_with(Vec::new).push(tx);
                }
            }
        }
        
        // Phát hiện các cặp token có nhiều giao dịch swap
        for (pair, txs) in token_pair_swaps.iter() {
            if txs.len() >= 3 {
                // Có thể có cơ hội arbitrage
                debug!("Phát hiện cặp token {} có {} giao dịch swap trong cùng block", pair, txs.len());
                
                // Kiểm tra giá trị chênh lệch giữa các giao dịch
                // TODO: Phân tích sâu hơn để xác định cơ hội arbitrage thực sự
            }
        }
        
        Ok(())
    }

    /// Phát hiện các giao dịch có thể sandwich
    async fn detect_sandwich_targets(&self, chain_id: u32, large_swaps: &[&MempoolTransaction]) -> Result<()> {
        // Lấy adapter cho chain này
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => {
                error!("Không tìm thấy adapter cho chain ID {}", chain_id);
                return Ok(());
            }
        };
        
        // Lọc các giao dịch swap lớn có thể là mục tiêu sandwich
        for tx in large_swaps {
            // Kiểm tra xem giao dịch có đủ lớn để đáng sandwich không
            if tx.value_usd < 1000.0 { // Ngưỡng tối thiểu $1000
                continue;
            }
            
            // Kiểm tra xem token có đủ thanh khoản không
            if let (Some(from_token), Some(to_token)) = (&tx.from_token, &tx.to_token) {
                // Kiểm tra token có đủ thanh khoản không
                let token_address = to_token.address.clone();
                
                // Lấy thông tin thanh khoản từ adapter
                let liquidity = match adapter.get_token_liquidity(&token_address).await {
                    Ok(liq) => liq,
                    Err(_) => continue, // Bỏ qua nếu không lấy được thông tin thanh khoản
                };
                
                // Kiểm tra xem giao dịch có đủ lớn để tạo price impact không
                let tx_value_pct_of_liquidity = tx.value_usd / liquidity * 100.0;
                
                // Nếu giao dịch tạo price impact đáng kể (>0.5%)
                if tx_value_pct_of_liquidity > 0.5 {
                    // Tính toán lợi nhuận tiềm năng
                    let estimated_profit_pct = tx_value_pct_of_liquidity * 0.1; // Ước tính lợi nhuận là 10% của price impact
                    let estimated_profit_usd = tx.value_usd * estimated_profit_pct / 100.0;
                    
                    // Ước tính lợi nhuận theo ETH
                    let profit_eth = estimated_profit_usd / to_token.price_usd.unwrap_or_else(|| {
                        warn!("Missing price_usd for token {}, using default value of 1800", token_address);
                        1800.0
                    });
                    
                    // Tạo cơ hội MEV
                    let opportunity = MevOpportunity {
                                                    id: format!("sandwich_{}_{}_{}", chain_id, tx.tx_hash.as_ref().map(|h| h.clone()).unwrap_or_default(), SystemTime::now().duration_since(UNIX_EPOCH).context("Lỗi khi lấy thời gian hiện tại")?.as_secs()),
                            opportunity_type: MevOpportunityType::Sandwich,
                            chain_id,
                            expected_profit: profit_eth,
                            estimated_net_profit_usd: estimated_profit_usd,
                            risk_score: 4,
                            execution_method: MevExecutionMethod::MevBundle,
                            status: MevOpportunityStatus::Detected,
                            timestamp: SystemTime::now().duration_since(UNIX_EPOCH).context("Lỗi khi lấy thời gian hiện tại")?.as_secs(),
                            expires_at: SystemTime::now().duration_since(UNIX_EPOCH).context("Lỗi khi tính thời gian hết hạn")?.as_secs() + 30, // 30 giây
                            actual_profit: None,
                            target_transaction: tx.tx_hash.clone(),
                            related_transactions: tx.tx_hash.clone().map_or_else(Vec::new, |hash| vec![hash]),
                        token_pair: Some(TokenPair {
                            token0: from_token.address.clone(),
                            token1: to_token.address.clone(),
                            base_token_address: from_token.address.clone(),
                            token_address: to_token.address.clone(),
                                            base_token: from_token.symbol.as_ref().map(|s| s.clone()).unwrap_or_else(|| "Unknown".to_string()),
                quote_token: to_token.symbol.as_ref().map(|s| s.clone()).unwrap_or_else(|| "Unknown".to_string()),
                            chain_id,
                        }),
                                                    specific_params: {
                                let mut params = HashMap::new();
                                params.insert("price_impact_pct".to_string(), tx_value_pct_of_liquidity.to_string());
                                if let Some(hash) = &tx.tx_hash {
                                    params.insert("target_tx".to_string(), hash.clone());
                                }
                                params.insert("liquidity".to_string(), liquidity.to_string());
                                params
                            },
                        bundle_data: None,
                        estimated_gas_cost_usd: 20.0, // Giá trị ước tính cao hơn vì cần 2 giao dịch
                        transaction_hash: None,
                    };
                    
                    // Thêm vào danh sách cơ hội
                    let mut opportunities = self.opportunities.write().await;
                    opportunities.push(opportunity);
                    
                    info!(
                        "Phát hiện cơ hội sandwich trên chain {}: giao dịch {} với price impact {}% cho token {}",
                        chain_id, 
                        tx.tx_hash.as_ref().map_or("<unknown>", |h| h.as_str()), 
                        tx_value_pct_of_liquidity, 
                        token_address
                    );
                }
            }
        }
        
        Ok(())
    }

    /// Phân tích các sự kiện thanh khoản
    async fn analyze_liquidity_events(&self, chain_id: u32, events: &[MempoolTransaction]) -> Result<()> {
        // Lấy adapter cho chain này
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => {
                error!("Không tìm thấy adapter cho chain ID {}", chain_id);
                return Ok(());
            }
        };
        
        // Nhóm các sự kiện theo token
        let mut token_events: HashMap<String, Vec<&MempoolTransaction>> = HashMap::new();
        
        for tx in events {
            if tx.transaction_type == TransactionType::AddLiquidity || tx.transaction_type == TransactionType::RemoveLiquidity {
                if let Some(token_address) = &tx.token_address {
                    token_events.entry(token_address.clone()).or_insert_with(Vec::new).push(tx);
                }
            }
        }
        
        // Phân tích các sự kiện theo token
        for (token_address, token_txs) in token_events {
            // Tính tổng giá trị thanh khoản được thêm/rút
            let mut total_add_liquidity = 0.0;
            let mut total_remove_liquidity = 0.0;
            
            for tx in &token_txs {
                match tx.transaction_type {
                    TransactionType::AddLiquidity => total_add_liquidity += tx.value_eth,
                    TransactionType::RemoveLiquidity => total_remove_liquidity += tx.value_eth,
                    _ => {}
                }
            }
            
            // Phát hiện các sự kiện đáng chú ý
            if total_remove_liquidity > total_add_liquidity && total_remove_liquidity > 5.0 {
                // Phát hiện rút thanh khoản lớn
                debug!("Phát hiện rút thanh khoản lớn cho token {}: {} ETH", token_address, total_remove_liquidity);
                
                // Kiểm tra xem có phải là rugpull không
                if total_remove_liquidity > 20.0 && total_add_liquidity < 1.0 {
                    warn!("Có thể là rugpull cho token {}: {} ETH bị rút", token_address, total_remove_liquidity);
                    
                    // Tạo cảnh báo rugpull
                    let alert = MempoolAlert {
                        alert_type: MempoolAlertType::SuspiciousActivity,
                        severity: AlertSeverity::High,
                        description: format!("Có thể rugpull cho token {}", token_address),
                        suspicious_pattern: Some(SuspiciousPattern::LiquidityRemoval),
                        target_transaction: None,
                        related_transactions: None,
                    };
                    
                    // Xử lý cảnh báo rút thanh khoản
                    self.process_liquidity_removal(chain_id, alert).await?;
                }
            } else if total_add_liquidity > 10.0 {
                // Phát hiện thêm thanh khoản lớn
                debug!("Phát hiện thêm thanh khoản lớn cho token {}: {} ETH", token_address, total_add_liquidity);
                
                // Kiểm tra xem token có phải mới không
                if let Ok(info) = adapter.get_token_info(&token_address).await {
                    if info.is_new_token {
                        info!("Phát hiện token mới với thanh khoản lớn: {} ({} ETH)", token_address, total_add_liquidity);
                        
                        // Tạo cảnh báo token mới
                        let alert = MempoolAlert {
                            alert_type: MempoolAlertType::NewToken,
                            severity: AlertSeverity::Medium,
                            description: format!("Token mới với thanh khoản lớn: {}", token_address),
                            suspicious_pattern: None,
                            id: Some(format!("new_token_{}", token_address)),
                            target_transaction: None,
                            related_transactions: None,
                        };
                        
                        // Xử lý cảnh báo thanh khoản mới
                        self.process_new_liquidity(chain_id, alert).await?;
                    }
                }
            }
        }
        
        Ok(())
    }

    /// Phân tích mối tương quan giữa approve và swap
    async fn correlate_approvals_and_swaps(&self, chain_id: u32, approvals: &[MempoolTransaction], swaps: &[MempoolTransaction]) -> Result<()> {
        // Tạo map từ địa chỉ người gửi đến các giao dịch approve
        let mut sender_approvals: HashMap<String, Vec<&MempoolTransaction>> = HashMap::new();
        
        for tx in approvals {
            // Kiểm tra function signature để xác định nếu đây là approve
            if let Some(sig) = &tx.function_signature {
                if sig.starts_with("0x095ea7b3") { // approve function signature
                    sender_approvals.entry(tx.from_address.clone()).or_insert_with(Vec::new).push(tx);
                }
            }
        }
        
        // Phân tích các giao dịch swap
        for tx in swaps {
            if tx.transaction_type == TransactionType::Swap {
                // Kiểm tra xem người gửi đã approve token chưa
                if let Some(sender_txs) = sender_approvals.get(&tx.from_address) {
                    // Tìm approve gần nhất
                    if let Some(approve_tx) = sender_txs.iter().max_by_key(|tx| tx.timestamp) {
                        // Kiểm tra thời gian giữa approve và swap
                        let time_diff = tx.timestamp.saturating_sub(approve_tx.timestamp);
                        
                        // Nếu thời gian giữa approve và swap quá ngắn (dưới 5 giây)
                        if time_diff < 5 {
                            debug!(
                                "Phát hiện approve-swap nhanh: {} approve sau đó swap trong {} giây",
                                tx.from_address, time_diff
                            );
                            
                            // Nếu giá trị swap lớn, có thể là bot
                            if tx.value_usd > 5000.0 {
                                info!(
                                    "Có thể là bot trading: {} approve và swap {} USD trong {} giây",
                                    tx.from_address, tx.value_usd, time_diff
                                );
                            }
                        }
                    }
                }
            }
        }
        
        Ok(())
    }

    /// Đánh giá các cơ hội đã phát hiện
    async fn evaluate_opportunities(&self) -> Result<()> {
        // Lấy danh sách cơ hội hiện tại
        let mut opportunities = self.opportunities.write().await;

        // Kiểm tra cơ hội nào đã hết hạn và loại bỏ
        let now = SystemTime::now().duration_since(UNIX_EPOCH).context("Lỗi khi lấy thời gian hiện tại")?.as_secs();
        opportunities.retain(|opp| {
            // Sử dụng expires_at
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
                    MevOpportunityType::BackRun => {
                        // Simulation code for backrun profitability
                        if let Some(target_tx) = &opp.target_transaction {
                            // Kiểm tra xem giao dịch gốc đã được thực hiện chưa
                            match adapter.get_transaction_status(target_tx).await {
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
                        },
                    },
                    MevOpportunityType::Sandwich => {
                        // Cập nhật profitability dựa trên trạng thái hiện tại của mempool
                        if let Some(target_tx) = &opp.target_transaction {
                            if let Some(analyzer) = self.mempool_analyzers.get(&opp.chain_id) {
                                // Check if transaction is still in mempool
                                match analyzer.is_transaction_in_mempool(target_tx).await {
                                    Ok(in_mempool) => {
                                        if !in_mempool {
                                            // Không còn trong mempool, có thể đã được thực hiện hoặc hết hạn
                                            opp.status = MevOpportunityStatus::Expired;
                                        }
                                    },
                                    Err(_) => {
                                        // Error when checking
                                        debug!("Error checking mempool status for transaction");
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
        opportunities.sort_by(|a, b| {
            match b.expected_profit.partial_cmp(&a.expected_profit) {
                Some(ordering) => ordering,
                None => std::cmp::Ordering::Equal
            }
        });
        
        Ok(())
    }

    /// Execute a transaction from the mempool
    ///
    /// # Arguments
    /// * `transaction` - The transaction to execute
    /// * `estimated_gas_cost_usd` - Estimated gas cost in USD
    ///
    /// # Returns
    /// * `Result<f64>` - Profit from the transaction (ETH)
    async fn execute_transaction(
        &self,
        chain_id: u32,
        transaction: &MempoolTransaction,
        estimated_gas_cost_usd: f64
    ) -> Result<f64> {
        let adapter = match self.evm_adapters.get(&chain_id) {
            Some(adapter) => adapter,
            None => {
                error!("Không tìm thấy adapter cho chain ID {}", chain_id);
                return Err(anyhow!("No adapter found for chain ID {}", chain_id));
            }
        };
        
        // Execute the transaction using the adapter
        adapter.execute_transaction(transaction, estimated_gas_cost_usd)
            .await
            .with_context(|| format!("Failed to execute transaction on chain {}", chain_id))
    }

    /// Thực hiện arbitrage
    async fn execute_arbitrage(&self, adapter: &Arc<EvmAdapter>, opportunity: &MevOpportunity) -> Result<String> {
        // Dựa vào phương thức thực thi
        match opportunity.execution_method {
            MevExecutionMethod::FlashLoan => {
                let profit = self.execute_flash_loan(opportunity, adapter).await
                    .context("Lỗi khi thực hiện flash loan")?;
                
                // Tạo hash giả cho ví dụ
                let tx_hash = format!("0xarbitrage_{}", opportunity.id);
                
                Ok(tx_hash)
            },
            MevExecutionMethod::Standard => {
                let profit = self.execute_standard_trade(opportunity, adapter).await
                    .context("Lỗi khi thực hiện giao dịch thông thường")?;
                
                // Tạo hash giả cho ví dụ
                let tx_hash = format!("0xarbitrage_std_{}", opportunity.id);
                
                Ok(tx_hash)
            },
            MevExecutionMethod::MultiSwap => {
                let profit = self.execute_multi_swap(opportunity, adapter).await
                    .context("Lỗi khi thực hiện multi swap")?;
                
                // Tạo hash giả cho ví dụ
                let tx_hash = format!("0xarbitrage_multi_{}", opportunity.id);
                
                Ok(tx_hash)
            },
            _ => {
                Err(anyhow!("Phương thức thực thi không được hỗ trợ cho arbitrage"))
            }
        }
    }

    /// Thực hiện sandwich
    async fn execute_sandwich(&self, adapter: &Arc<EvmAdapter>, opportunity: &MevOpportunity) -> Result<String> {
        // Dựa vào phương thức thực thi
        match opportunity.execution_method {
            MevExecutionMethod::MevBundle => {
                let profit = self.execute_mev_bundle(opportunity, adapter).await
                    .context("Lỗi khi thực hiện MEV bundle")?;
                
                // Tạo hash giả cho ví dụ
                let tx_hash = format!("0xsandwich_{}", opportunity.id);
                
                Ok(tx_hash)
            },
            _ => {
                Err(anyhow!("Phương thức thực thi không được hỗ trợ cho sandwich"))
            }
        }
    }

    /// Thực hiện front-running
    async fn execute_frontrun(&self, adapter: &Arc<EvmAdapter>, opportunity: &MevOpportunity) -> Result<String> {
        // Dựa vào phương thức thực thi
        match opportunity.execution_method {
            MevExecutionMethod::PrivateRPC => {
                let profit = self.execute_private_rpc(opportunity, adapter).await
                    .context("Lỗi khi thực hiện private RPC")?;
                
                // Tạo hash giả cho ví dụ
                let tx_hash = format!("0xfrontrun_{}", opportunity.id);
                
                Ok(tx_hash)
            },
            MevExecutionMethod::BuilderAPI => {
                let profit = self.execute_builder_api(opportunity, adapter).await
                    .context("Lỗi khi thực hiện builder API")?;
                
                // Tạo hash giả cho ví dụ
                let tx_hash = format!("0xfrontrun_builder_{}", opportunity.id);
                
                Ok(tx_hash)
            },
            _ => {
                Err(anyhow!("Phương thức thực thi không được hỗ trợ cho front-running"))
            }
        }
    }

    /// Thực hiện back-running
    async fn execute_backrun(&self, adapter: &Arc<EvmAdapter>, opportunity: &MevOpportunity) -> Result<String> {
        // Dựa vào phương thức thực thi
        match opportunity.execution_method {
            MevExecutionMethod::Standard => {
                let profit = self.execute_standard_trade(opportunity, adapter).await
                    .context("Lỗi khi thực hiện giao dịch thông thường")?;
                
                // Tạo hash giả cho ví dụ
                let tx_hash = format!("0xbackrun_{}", opportunity.id);
                
                Ok(tx_hash)
            },
            _ => {
                Err(anyhow!("Phương thức thực thi không được hỗ trợ cho back-running"))
            }
        }
    }

    /// Thực hiện snipe token mới
    async fn execute_new_token_snipe(&self, adapter: &Arc<EvmAdapter>, opportunity: &MevOpportunity) -> Result<String> {
        // Dựa vào phương thức thực thi
        match opportunity.execution_method {
            MevExecutionMethod::Standard => {
                let profit = self.execute_standard_trade(opportunity, adapter).await
                    .context("Lỗi khi thực hiện giao dịch thông thường")?;
                
                // Tạo hash giả cho ví dụ
                let tx_hash = format!("0xsnipe_{}", opportunity.id);
                
                Ok(tx_hash)
            },
            _ => {
                Err(anyhow!("Phương thức thực thi không được hỗ trợ cho snipe token mới"))
            }
        }
    }

    /// Thực hiện JIT liquidity
    async fn execute_jit_liquidity(&self, adapter: &Arc<EvmAdapter>, opportunity: &MevOpportunity) -> Result<String> {
        // Dựa vào phương thức thực thi
        match opportunity.execution_method {
            MevExecutionMethod::CustomContract => {
                let profit = self.execute_custom_contract(opportunity, adapter).await
                    .context("Lỗi khi thực hiện custom contract")?;
                
                // Tạo hash giả cho ví dụ
                let tx_hash = format!("0xjit_{}", opportunity.id);
                
                Ok(tx_hash)
            },
            _ => {
                Err(anyhow!("Phương thức thực thi không được hỗ trợ cho JIT liquidity"))
            }
        }
    }

    /// Thực hiện cross-domain
    async fn execute_cross_domain(&self, adapter: &Arc<EvmAdapter>, opportunity: &MevOpportunity) -> Result<String> {
        // Dựa vào phương thức thực thi
        match opportunity.execution_method {
            MevExecutionMethod::CustomContract => {
                let profit = self.execute_custom_contract(opportunity, adapter).await
                    .context("Lỗi khi thực hiện custom contract")?;
                
                // Tạo hash giả cho ví dụ
                let tx_hash = format!("0xcrossdomain_{}", opportunity.id);
                
                Ok(tx_hash)
            },
            _ => {
                Err(anyhow!("Phương thức thực thi không được hỗ trợ cho cross-domain"))
            }
        }
    }
} 