//! Module triển khai chức năng bridge cho token DMD trên các blockchain.
//! Sử dụng mô hình hub-and-spoke với NEAR Protocol là hub trung tâm.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet, BTreeMap, VecDeque};
use std::sync::{Arc, RwLock};
use uuid::Uuid;
use lru::LruCache;
use regex::Regex;
use ethers::types::U256;
use serde::{Deserialize, Serialize};
use log::{debug, error, info, warn};
use thiserror::Error;
use tokio::sync::Mutex;
use std::time::{Duration, Instant};

use crate::blockchain::BlockchainProvider;
use crate::bridge::error::{BridgeError, BridgeResult, is_evm_chain};
use crate::smartcontracts::dmd_token::DmdChain;

/// Trạng thái của một giao dịch bridge
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeTransactionStatus {
    /// Giao dịch được tạo nhưng chưa bắt đầu
    Created,
    /// Giao dịch đang xử lý
    InProgress,
    /// Giao dịch hoàn thành
    Completed,
    /// Giao dịch thất bại
    Failed,
}

/// Thông tin một giao dịch bridge
#[derive(Debug, Clone)]
pub struct BridgeTransaction {
    /// ID giao dịch
    pub id: String,
    /// Chain nguồn
    pub source_chain: DmdChain,
    /// Chain đích
    pub target_chain: DmdChain,
    /// Địa chỉ nguồn
    pub source_address: String,
    /// Địa chỉ đích
    pub target_address: String,
    /// Số lượng token
    pub amount: u128,
    /// Hash giao dịch trên chain nguồn
    pub source_tx_hash: Option<String>,
    /// Hash giao dịch trên chain đích
    pub target_tx_hash: Option<String>,
    /// Trạng thái
    pub status: BridgeTransactionStatus,
    /// Thời gian tạo
    pub created_at: DateTime<Utc>,
    /// Thời gian cập nhật
    pub updated_at: DateTime<Utc>,
    /// Thông tin lỗi (nếu có)
    pub error: Option<String>,
}

/// Trait định nghĩa adapter cho bridge
#[async_trait]
pub trait BridgeAdapter: Send + Sync {
    /// Trả về tên của adapter
    fn get_adapter_name(&self) -> &str;
    
    /// Trả về danh sách các chain được hỗ trợ
    fn get_supported_chains(&self) -> Vec<DmdChain>;
    
    /// Kiểm tra xem adapter có hỗ trợ route này không
    fn supports_route(&self, source: &DmdChain, target: &DmdChain) -> bool;
    
    /// Chuyển token từ chain nguồn sang chain đích
    async fn transfer(
        &self,
        tx_id: &str,
        source_chain: &DmdChain,
        target_chain: &DmdChain,
        source_address: &str,
        target_address: &str,
        amount: u128,
    ) -> BridgeResult<String>;
    
    /// Kiểm tra trạng thái giao dịch
    async fn check_transaction_status(
        &self,
        tx_id: &str,
        source_chain: &DmdChain,
        target_chain: &DmdChain,
        source_tx_hash: &str,
    ) -> BridgeResult<BridgeTransactionStatus>;
}

/// Adapter sử dụng LayerZero cho các EVM chain
pub struct LayerZeroAdapter {
    /// Các chain được hỗ trợ
    supported_chains: HashSet<DmdChain>,
    /// Provider factory
    provider_factory: Arc<dyn BlockchainProvider>,
    /// Ánh xạ từ chain sang LayerZero chain ID
    chain_to_lz_id: HashMap<DmdChain, u16>,
}

impl LayerZeroAdapter {
    /// Tạo mới adapter LayerZero
    pub fn new(provider_factory: Arc<dyn BlockchainProvider>) -> Self {
        let mut supported_chains = HashSet::new();
        supported_chains.insert(DmdChain::Ethereum);
        supported_chains.insert(DmdChain::BinanceSmartChain);
        supported_chains.insert(DmdChain::Avalanche);
        supported_chains.insert(DmdChain::Polygon);
        supported_chains.insert(DmdChain::Arbitrum);
        supported_chains.insert(DmdChain::Optimism);
        supported_chains.insert(DmdChain::Base);
        
        let mut chain_to_lz_id = HashMap::new();
        chain_to_lz_id.insert(DmdChain::Ethereum, 101);
        chain_to_lz_id.insert(DmdChain::BinanceSmartChain, 102);
        chain_to_lz_id.insert(DmdChain::Avalanche, 106);
        chain_to_lz_id.insert(DmdChain::Polygon, 109);
        chain_to_lz_id.insert(DmdChain::Arbitrum, 110);
        chain_to_lz_id.insert(DmdChain::Optimism, 111);
        chain_to_lz_id.insert(DmdChain::Base, 184);
        
        Self {
            supported_chains,
            provider_factory,
            chain_to_lz_id,
        }
    }
    
    /// Trả về LayerZero ID của chain
    fn get_lz_chain_id(&self, chain: &DmdChain) -> BridgeResult<u16> {
        self.chain_to_lz_id.get(chain)
            .copied()
            .ok_or_else(|| BridgeError::UnsupportedChain(format!("Chain không hỗ trợ LayerZero: {:?}", chain)))
    }

    /// Chuyển token từ chuỗi nguồn đến chuỗi đích
    pub async fn transfer(&self, transaction: &BridgeTransaction) -> BridgeResult<String> {
        // Xác thực giao dịch trước khi thực hiện
        self.validate_transaction(transaction).await?;
        
        // Kiểm tra xem cặp chuỗi này có được hỗ trợ không
        if !self.supports_route(&transaction.source_chain, &transaction.target_chain) {
            return Err(BridgeError::UnsupportedChain(format!(
                "Route từ {} đến {} không được hỗ trợ",
                transaction.source_chain, transaction.target_chain
            )));
        }
        
        // Lấy id chuỗi đích
        let destination_chain_id = self.get_lz_chain_id(&transaction.target_chain)?;
        
        // Lấy provider cho chuỗi nguồn
        let provider = match self.get_provider(&transaction.source_chain) {
            Some(p) => p,
            None => return Err(BridgeError::ProviderError(format!(
                "Không tìm thấy provider cho chuỗi {}",
                transaction.source_chain
            ))),
        };
        
        // Chuẩn bị dữ liệu
        let adapter_data = self.prepare_adapter_data(transaction, destination_chain_id)?;
        
        // Xử lý kiểm tra hạn ngạch và đánh giá rủi ro giao dịch
        self.evaluate_transaction_risk(transaction).await?;
        
        // Tạo và gửi giao dịch thông qua adapter
        info!("Chuyển {} token từ {} đến {}", 
            transaction.amount, transaction.source_chain, transaction.target_chain);
            
        // Gọi hợp đồng bridge thông qua provider
        let tx_hash = provider.bridge_token(
            &transaction.source_address,
            &transaction.target_address,
            &transaction.amount,
            destination_chain_id,
            &adapter_data,
        ).await?;
        
        // Ghi log và trả về hash giao dịch
        info!("Khởi tạo giao dịch bridge thành công với hash: {}", tx_hash);
        Ok(tx_hash)
    }
    
    /// Xác thực giao dịch trước khi thực hiện
    async fn validate_transaction(&self, transaction: &BridgeTransaction) -> BridgeResult<()> {
        // Kiểm tra số lượng
        if transaction.amount.parse::<f64>().unwrap_or(0.0) <= 0.0 {
            return Err(BridgeError::InvalidAmount(transaction.amount.clone()));
        }
        
        // Kiểm tra địa chỉ nguồn và đích
        if transaction.source_address.len() < 10 {
            return Err(BridgeError::InvalidAddress(format!(
                "Địa chỉ nguồn không hợp lệ: {}",
                transaction.source_address
            )));
        }
        
        if transaction.target_address.len() < 10 {
            return Err(BridgeError::InvalidAddress(format!(
                "Địa chỉ đích không hợp lệ: {}",
                transaction.target_address
            )));
        }
        
        // Kiểm tra định dạng địa chỉ theo từng chuỗi
        self.validate_address_format(&transaction.source_address, &transaction.source_chain)?;
        self.validate_address_format(&transaction.target_address, &transaction.target_chain)?;
        
        // Kiểm tra xem token có được hỗ trợ trên chuỗi đích không
        self.validate_token_support(&transaction.target_chain)?;
        
        // Kiểm tra giới hạn số lượng
        self.validate_amount_limits(transaction).await?;
        
        Ok(())
    }
    
    /// Kiểm tra định dạng địa chỉ theo từng chuỗi
    fn validate_address_format(&self, address: &str, chain: &str) -> BridgeResult<()> {
        match chain.to_lowercase().as_str() {
            "ethereum" | "bsc" | "polygon" | "aurora" => {
                // Kiểm tra định dạng địa chỉ ETH (0x...)
                if !address.starts_with("0x") || address.len() != 42 {
                    return Err(BridgeError::InvalidAddress(format!(
                        "Địa chỉ {} không hợp lệ cho chuỗi {}", address, chain
                    )));
                }
            },
            "near" => {
                // Kiểm tra định dạng địa chỉ NEAR
                if address.len() < 2 || address.len() > 64 || !address.contains('.') {
                    return Err(BridgeError::InvalidAddress(format!(
                        "Địa chỉ {} không hợp lệ cho chuỗi {}", address, chain
                    )));
                }
            },
            _ => {
                // Đối với các chuỗi khác, thực hiện kiểm tra chung
                if address.len() < 10 {
                    return Err(BridgeError::InvalidAddress(format!(
                        "Địa chỉ {} không hợp lệ", address
                    )));
                }
            }
        }
        
        Ok(())
    }
    
    /// Kiểm tra giới hạn số lượng cho từng chuỗi
    async fn validate_amount_limits(&self, transaction: &BridgeTransaction) -> BridgeResult<()> {
        let amount = match transaction.amount.parse::<f64>() {
            Ok(a) => a,
            Err(_) => return Err(BridgeError::InvalidAmount(transaction.amount.clone()))
        };
        
        // Lấy giới hạn cho cặp chuỗi
        let (min_limit, max_limit) = self.get_chain_limits(&transaction.source_chain, &transaction.target_chain)?;
        
        if amount < min_limit {
            return Err(BridgeError::InvalidAmount(format!(
                "Số lượng ({}) thấp hơn giới hạn tối thiểu ({}) cho cặp chuỗi {}-{}",
                amount, min_limit, transaction.source_chain, transaction.target_chain
            )));
        }
        
        if amount > max_limit {
            return Err(BridgeError::InvalidAmount(format!(
                "Số lượng ({}) vượt quá giới hạn tối đa ({}) cho cặp chuỗi {}-{}",
                amount, max_limit, transaction.source_chain, transaction.target_chain
            )));
        }
        
        Ok(())
    }
    
    /// Lấy giới hạn cho cặp chuỗi
    fn get_chain_limits(&self, source_chain: &str, target_chain: &str) -> BridgeResult<(f64, f64)> {
        // Giá trị mặc định
        let default_min = 0.01;
        let default_max = 1_000_000.0;
        
        // Giới hạn tùy chỉnh cho từng cặp chuỗi
        let limits = match (source_chain.to_lowercase().as_str(), target_chain.to_lowercase().as_str()) {
            ("ethereum", "near") => (0.05, 500_000.0),
            ("near", "ethereum") => (0.1, 300_000.0),
            ("ethereum", "bsc") => (0.01, 1_000_000.0),
            ("bsc", "ethereum") => (0.01, 800_000.0),
            ("polygon", "ethereum") => (0.02, 700_000.0),
            ("ethereum", "polygon") => (0.02, 900_000.0),
            ("near", "aurora") => (0.01, 1_200_000.0),
            ("aurora", "near") => (0.01, 1_200_000.0),
            _ => (default_min, default_max),
        };
        
        Ok(limits)
    }
    
    /// Đánh giá rủi ro giao dịch
    async fn evaluate_transaction_risk(&self, transaction: &BridgeTransaction) -> BridgeResult<()> {
        // Lấy oracle để kiểm tra tính bất thường
        let oracle = match &self.oracle {
            Some(o) => o,
            None => return Ok(()) // Nếu không có oracle, bỏ qua bước này
        };
        
        // Kiểm tra tính bất thường của giao dịch
        let is_abnormal = oracle.detect_abnormal_transaction(
            &transaction.source_chain,
            &transaction.target_chain,
            &transaction.amount,
            &transaction.source_address
        ).await?;
        
        if is_abnormal {
            return Err(BridgeError::SecurityRisk(
                "Giao dịch có dấu hiệu bất thường và bị từ chối".into()
            ));
        }
        
        // Kiểm tra xác thực chéo giao dịch
        let cross_validation = oracle.cross_validate_bridge_transaction(
            transaction.id.as_ref().unwrap_or(&"".to_string()),
            &transaction.source_chain,
            &transaction.target_chain,
            &transaction.source_address,
            &transaction.target_address,
            &transaction.amount
        ).await?;
        
        if let Some(risk) = cross_validation {
            return Err(BridgeError::SecurityRisk(format!(
                "Xác thực chéo giao dịch không thành công: {}", risk
            )));
        }
        
        Ok(())
    }
    
    /// Kiểm tra xem token có được hỗ trợ trên chuỗi đích không
    fn validate_token_support(&self, target_chain: &str) -> BridgeResult<()> {
        // Danh sách chuỗi hỗ trợ token DMD
        let supported_chains = vec![
            "ethereum", "bsc", "polygon", "near", "aurora"
        ];
        
        if !supported_chains.contains(&target_chain.to_lowercase().as_str()) {
            return Err(BridgeError::UnsupportedChain(format!(
                "Token DMD không được hỗ trợ trên chuỗi {}", target_chain
            )));
        }
        
        Ok(())
    }
}

#[async_trait]
impl BridgeAdapter for LayerZeroAdapter {
    fn get_adapter_name(&self) -> &str {
        "LayerZero"
    }
    
    fn get_supported_chains(&self) -> Vec<DmdChain> {
        self.supported_chains.iter().cloned().collect()
    }
    
    fn supports_route(&self, source: &DmdChain, target: &DmdChain) -> bool {
        self.supported_chains.contains(source) && self.supported_chains.contains(target)
    }
    
    async fn transfer(
        &self,
        tx_id: &str,
        source_chain: &DmdChain,
        target_chain: &DmdChain,
        source_address: &str,
        target_address: &str,
        amount: u128,
    ) -> BridgeResult<String> {
        if !self.supports_route(source_chain, target_chain) {
            return Err(BridgeError::UnsupportedRoute(
                format!("LayerZero không hỗ trợ route từ {:?} đến {:?}", source_chain, target_chain)
            ));
        }
        
        // Lấy LayerZero ID cho chain đích
        let target_lz_id = self.get_lz_chain_id(target_chain)?;
        
        // Lấy provider cho chain nguồn
        let provider = self.provider_factory.clone();
        
        // Khởi tạo giao dịch bridge (giả lập)
        // Trong triển khai thực tế, đây sẽ là một lệnh gọi hợp đồng LayerZero
        let tx_hash = format!("0x{:x}", Uuid::new_v4().as_u128());
        
        // Ghi log
        log::info!(
            "Bridge transaction created: ID={}, Source={:?}, Target={:?}, Amount={}, Hash={}",
            tx_id, source_chain, target_chain, amount, tx_hash
        );
        
        Ok(tx_hash)
    }
    
    async fn check_transaction_status(
        &self,
        tx_id: &str,
        source_chain: &DmdChain,
        target_chain: &DmdChain,
        source_tx_hash: &str,
    ) -> BridgeResult<BridgeTransactionStatus> {
        // Kiểm tra xem route có được hỗ trợ không
        if !self.supports_route(source_chain, target_chain) {
            return Err(BridgeError::UnsupportedRoute(
                format!("LayerZero không hỗ trợ route từ {:?} đến {:?}", source_chain, target_chain)
            ));
        }
        
        // Kiểm tra tính hợp lệ của tx_hash
        if source_tx_hash.is_empty() || !source_tx_hash.starts_with("0x") {
            return Err(BridgeError::InvalidTransactionHash(format!(
                "Transaction hash không hợp lệ: {}",
                source_tx_hash
            )));
        }
        
        // Lấy provider
        let provider = self.provider_factory.clone();
        
        // Lấy thông tin chain để kiểm tra adapter tương thích
        let chain_info = match source_chain {
            DmdChain::Ethereum => "ethereum",
            DmdChain::BinanceSmartChain => "bsc",
            DmdChain::Avalanche => "avalanche",
            DmdChain::Polygon => "polygon",
            DmdChain::Arbitrum => "arbitrum",
            DmdChain::Optimism => "optimism",
            DmdChain::Base => "base",
            // Xử lý cho các adapter không tương thích
            DmdChain::Near | DmdChain::Solana | _ => {
                debug!("Sử dụng fallback cho chain không phải EVM: {:?}", source_chain);
                // Giải pháp tạm thời cho các chain không phải EVM
                // Trong triển khai thực tế, sẽ cần một adapter riêng cho mỗi chain
                return Ok(BridgeTransactionStatus::InProgress);
            }
        };
        
        // Trong triển khai thực tế, sẽ kiểm tra trạng thái giao dịch trên bridge contract
        // Dựa vào chain đã kiểm tra
        match chain_info {
            "bsc" | "ethereum" | "polygon" | "arbitrum" | "avalanche" | "optimism" | "base" => {
                // Kiểm tra transaction receipt cho các chain EVM
                debug!("Kiểm tra trạng thái giao dịch trên chain {}: {}", chain_info, source_tx_hash);
                
                // Giả lập: Kiểm tra xem transaction đã được xác nhận chưa
                // Trong triển khai thực tế, sẽ gọi API JSON-RPC để kiểm tra
                let confirmed = true; // Mock kết quả
                
                if confirmed {
                    Ok(BridgeTransactionStatus::Completed)
                } else {
                    Ok(BridgeTransactionStatus::InProgress)
                }
            },
            _ => {
                // Không bao giờ đi đến đây do đã xử lý ở trên, nhưng giữ để đảm bảo tính hoàn chỉnh
                warn!("Không tìm thấy cách kiểm tra cho chain {}", chain_info);
                Ok(BridgeTransactionStatus::InProgress)
            }
        }
    }
}

/// Bridge Manager - quản lý các bridge adapter và giao dịch
pub struct BridgeManager {
    /// Bridge adapters for different chains
    adapters: Vec<Box<dyn BridgeAdapter>>,
    /// Cache of adapters by chain (in-memory)
    adapter_cache: Arc<RwLock<HashMap<DmdChain, usize>>>,
    /// Ánh xạ trực tiếp từ chain đến adapter thông qua HashMap
    adapter_map: Arc<RwLock<HashMap<DmdChain, usize>>>,
    /// Transaction repository
    tx_repository: Arc<dyn BridgeTransactionRepository + Send + Sync>,
    /// Cache of transactions (in-memory)
    tx_cache: RwLock<LruCache<String, BridgeTransaction>>,
    /// Hàng đợi cập nhật hàng loạt cho giao dịch
    batch_update_queue: Arc<Mutex<VecDeque<BridgeTransaction>>>,
    /// Thời gian lần cập nhật hàng loạt cuối cùng
    last_batch_update: Arc<RwLock<Instant>>,
    /// Kích thước lô tối đa
    max_batch_size: usize,
    /// Thời gian tối đa giữa các lần cập nhật (ms)
    max_batch_interval_ms: u64,
}

impl BridgeManager {
    /// Tạo BridgeManager mới
    pub fn new() -> Self {
        let adapter_cache = Arc::new(RwLock::new(HashMap::new()));
        let adapter_map = Arc::new(RwLock::new(HashMap::new()));
        let tx_cache = RwLock::new(LruCache::new(1000));
        let batch_update_queue = Arc::new(Mutex::new(VecDeque::new()));
        let last_batch_update = Arc::new(RwLock::new(Instant::now()));
        
        let manager = Self {
            adapters: Vec::new(),
            adapter_cache,
            adapter_map,
            tx_repository: Arc::new(crate::bridge::memory_repository::InMemoryBridgeTransactionRepository::new()),
            tx_cache,
            batch_update_queue,
            last_batch_update,
            max_batch_size: 50,
            max_batch_interval_ms: 1000, // 1 giây
        };
        
        // Khởi động worker xử lý hàng loạt
        manager.start_batch_processor();
        
        manager
    }
    
    /// Tạo BridgeManager với repository cụ thể
    pub fn with_repository(tx_repository: Arc<dyn BridgeTransactionRepository + Send + Sync>) -> Self {
        let adapter_cache = Arc::new(RwLock::new(HashMap::new()));
        let adapter_map = Arc::new(RwLock::new(HashMap::new()));
        let tx_cache = RwLock::new(LruCache::new(1000));
        let batch_update_queue = Arc::new(Mutex::new(VecDeque::new()));
        let last_batch_update = Arc::new(RwLock::new(Instant::now()));
        
        let manager = Self {
            adapters: Vec::new(),
            adapter_cache,
            adapter_map,
            tx_repository,
            tx_cache,
            batch_update_queue,
            last_batch_update,
            max_batch_size: 50,
            max_batch_interval_ms: 1000, // 1 giây
        };
        
        // Khởi động worker xử lý hàng loạt
        manager.start_batch_processor();
        
        manager
    }
    
    /// Khởi động worker xử lý hàng loạt cho cập nhật giao dịch
    fn start_batch_processor(&self) {
        let batch_queue = self.batch_update_queue.clone();
        let tx_repository = self.tx_repository.clone();
        let tx_cache = Arc::new(self.tx_cache.clone());
        let last_batch_update = self.last_batch_update.clone();
        let max_batch_size = self.max_batch_size;
        let max_batch_interval = Duration::from_millis(self.max_batch_interval_ms);
        
        tokio::spawn(async move {
            debug!("Khởi động worker xử lý hàng loạt cho cập nhật giao dịch");
            
            loop {
                // Kiểm tra thời gian chờ và kích thước hàng đợi
                let should_process = {
                    let mut last_update = last_batch_update.write().unwrap();
                    let queue = batch_queue.lock().await;
                    
                    let queue_size = queue.len();
                    let elapsed = last_update.elapsed();
                    
                    // Xử lý nếu:
                    // 1. Hàng đợi đạt kích thước tối đa, hoặc
                    // 2. Đã quá thời gian tối đa kể từ lần cập nhật cuối và có ít nhất 1 giao dịch trong hàng đợi
                    let should_process = queue_size >= max_batch_size || 
                                        (queue_size > 0 && elapsed >= max_batch_interval);
                    
                    if should_process {
                        *last_update = Instant::now();
                    }
                    
                    should_process
                };
                
                if should_process {
                    // Lấy tất cả giao dịch từ hàng đợi
                    let mut transactions = {
                        let mut queue = batch_queue.lock().await;
                        let mut transactions = Vec::with_capacity(queue.len());
                        
                        while let Some(tx) = queue.pop_front() {
                            transactions.push(tx);
                            if transactions.len() >= max_batch_size {
                                break;
                            }
                        }
                        
                        transactions
                    };
                    
                    if !transactions.is_empty() {
                        debug!("Xử lý hàng loạt {} giao dịch", transactions.len());
                        
                        // Xử lý hàng loạt
                        match tx_repository.save_batch(&transactions) {
                            Ok(()) => {
                                // Cập nhật cache
                                let mut cache = tx_cache.write().unwrap();
                                for tx in transactions {
                                    cache.put(tx.id.clone(), tx);
                                }
                                debug!("Cập nhật hàng loạt thành công {} giao dịch", transactions.len());
                            },
                            Err(e) => {
                                error!("Lỗi khi lưu hàng loạt giao dịch: {}", e);
                                // Xử lý từng giao dịch riêng lẻ để tránh mất dữ liệu
                                for tx in transactions {
                                    if let Err(e) = tx_repository.save(&tx) {
                                        error!("Không thể lưu giao dịch {}: {}", tx.id, e);
                                    } else {
                                        let mut cache = tx_cache.write().unwrap();
                                        cache.put(tx.id.clone(), tx);
                                    }
                                }
                            }
                        }
                    }
                }
                
                // Ngủ một chút để giảm tải CPU
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });
    }

    /// Đăng ký một adapter mới
    pub fn register_adapter(&mut self, adapter: Box<dyn BridgeAdapter>) {
        // Lấy các chain được hỗ trợ bởi adapter
        let supported_chains = adapter.get_supported_chains();
        
        // Lấy vị trí của adapter trong vector
        let adapter_index = self.adapters.len();
        
        // Thêm adapter vào danh sách
        self.adapters.push(adapter);
        
        // Cập nhật map trực tiếp với các chain được hỗ trợ
        let mut adapter_map = self.adapter_map.write().unwrap();
        
        for chain in supported_chains {
            adapter_map.insert(chain, adapter_index);
        }
        
        debug!("Đã đăng ký adapter tại index {}, hỗ trợ {} chains", adapter_index, adapter_map.len());
    }
    
    /// Find a suitable adapter for a chain - phiên bản tối ưu hóa
    fn find_adapter(&self, chain: DmdChain) -> Option<&Box<dyn BridgeAdapter>> {
        // Sử dụng adapter_map
        let adapter_map = self.adapter_map.read().unwrap();
        
        if let Some(&index) = adapter_map.get(&chain) {
            // Adapter được tìm thấy trực tiếp trong map
            return self.adapters.get(index);
        }
        
        // Nếu không tìm thấy trong map, kiểm tra cache cũ
        let cache = self.adapter_cache.read().unwrap();
        if let Some(&index) = cache.get(&chain) {
            // Adapter tìm thấy trong cache cũ
            return self.adapters.get(index);
        }
        
        // Không tìm thấy trong cả map và cache, phải tìm kiếm theo cách cũ
        // Đánh dấu thời gian bắt đầu tìm kiếm để đo hiệu suất
        let start = std::time::Instant::now();
        
        // Tìm kiếm adapter phù hợp
        let adapter_pos = self.adapters.iter().position(|a| a.get_supported_chains().contains(&chain));
        
        if let Some(index) = adapter_pos {
            // Cập nhật cả hai map
            drop(adapter_map); // Giải phóng read lock
            drop(cache); // Giải phóng read lock
            
            // Cập nhật map chính
            {
                let mut adapter_map = self.adapter_map.write().unwrap();
                adapter_map.insert(chain, index);
            }
            
            // Cập nhật cache cũ (để duy trì tương thích)
            {
                let mut cache = self.adapter_cache.write().unwrap();
                cache.insert(chain, index);
            }
            
            // Log thông tin hiệu suất
            let duration = start.elapsed();
            if duration.as_millis() > 5 {
                // Log cảnh báo nếu tìm kiếm mất quá 5ms
                warn!("Tìm kiếm adapter cho chain {:?} mất {}ms", chain, duration.as_millis());
            }
            
            return self.adapters.get(index);
        }
        
        // Không tìm thấy adapter nào phù hợp
        let duration = start.elapsed();
        warn!("Không tìm thấy adapter nào cho chain {:?} sau {}ms", chain, duration.as_millis());
        None
    }

    /// Create a new bridge transaction
    pub async fn create_transaction(&self, source_chain: DmdChain, target_chain: DmdChain, 
                           source_address: &str, target_address: &str, 
                           amount: U256, fee: U256) -> Result<BridgeTransaction, String> {
        // Create the transaction
        let tx = BridgeTransaction::new(
            source_chain,
            target_chain,
            source_address.to_string(),
            target_address.to_string(),
            amount,
            fee,
        );
        
        // Save to the transaction repository
        self.tx_repository.save(&tx)
            .map_err(|e| format!("Không thể lưu giao dịch: {}", e))?;
        
        // Cache the transaction
        let mut cache = self.tx_cache.write().unwrap();
        cache.put(tx.id.clone(), tx.clone());
        
        Ok(tx)
    }
    
    /// Bridge tokens from one chain to another
    pub async fn bridge(&self, 
                source_chain: DmdChain, 
                target_chain: DmdChain,
                private_key: &str,
                target_address: &str,
                amount: U256) -> Result<BridgeTransaction, String> {
        
        // Validate that the bridge is supported
        if !crate::bridge::is_bridge_supported(&source_chain, &target_chain) {
            return Err(format!("Bridge không được hỗ trợ từ {:?} đến {:?}", source_chain, target_chain));
        }
        
        // Find the source adapter
        let source_adapter = self.find_adapter(source_chain)
            .ok_or_else(|| format!("Không tìm thấy adapter cho chain {:?}", source_chain))?;
        
        // Find the target adapter
        let target_adapter = self.find_adapter(target_chain)
            .ok_or_else(|| format!("Không tìm thấy adapter cho chain {:?}", target_chain))?;
        
        // Get the source address from the private key
        let source_address = source_adapter.get_address_from_key(private_key)
            .map_err(|e| format!("Không thể lấy địa chỉ từ private key: {}", e))?;
        
        // Estimate the fee
        let fee = source_adapter.estimate_fee(target_chain, amount)
            .map_err(|e| format!("Không thể ước tính phí: {}", e))?;
        
        // Create a transaction
        let tx = self.create_transaction(
            source_chain,
            target_chain,
            &source_address,
            target_address,
            amount,
            fee,
        ).await?;
        
        // Perform the bridge
        let result = source_adapter.transfer_to(
            target_chain,
            private_key,
            target_address,
            amount,
            fee,
        ).map_err(|e| format!("Không thể thực hiện bridge: {}", e))?;
        
        // Update the transaction
        self.update_transaction(&tx.id, BridgeStatus::Processing, Some(result.tx_hash.clone()), None)
            .await
            .map_err(|e| format!("Không thể cập nhật giao dịch: {}", e))?;
        
        // Return the updated transaction
        self.get_transaction(&tx.id).await
    }
    
    /// Check the status of a bridge transaction
    pub async fn check_transaction_status(&self, tx_id: &str) -> Result<BridgeStatus, String> {
        // Get the transaction
        let tx = self.get_transaction(tx_id).await?;
        
        // Find the appropriate adapter
        let adapter = self.find_adapter(tx.source_chain)
            .ok_or_else(|| format!("Không tìm thấy adapter cho chain {:?}", tx.source_chain))?;
        
        // Check the status
        let status = adapter.check_transaction_status(&tx)
            .map_err(|e| format!("Không thể kiểm tra trạng thái giao dịch: {}", e))?;
        
        // Update the transaction if the status has changed
        if status != tx.status {
            self.update_transaction(tx_id, status, None, None).await?;
        }
        
        Ok(status)
    }
    
    /// Update a transaction
    pub async fn update_transaction(&self, tx_id: &str, 
                           status: BridgeStatus, 
                           source_tx_hash: Option<String>,
                           target_tx_hash: Option<String>) -> Result<BridgeTransaction, String> {
        // Get the current transaction
        let mut tx = match self.get_transaction(tx_id).await {
            Ok(tx) => tx,
            Err(e) => return Err(format!("Không thể lấy thông tin giao dịch: {}", e)),
        };
        
        // Update the status
        tx.status = status;
        
        // Update the transaction hashes
        if let Some(hash) = source_tx_hash {
            tx.source_tx_hash = Some(hash);
        }
        
        if let Some(hash) = target_tx_hash {
            tx.target_tx_hash = Some(hash);
        }
        
        // Update the timestamps
        tx.updated_at = chrono::Utc::now();
        
        if status == BridgeStatus::Completed || status == BridgeStatus::Failed {
            tx.completed_at = Some(chrono::Utc::now());
        }
        
        // Queue the transaction for batch update
        self.queue_transaction_update(tx.clone()).await;
        
        // Cập nhật cache luôn để đảm bảo có kết quả ngay lập tức
        {
            let mut cache = self.tx_cache.write().unwrap();
            cache.put(tx.id.clone(), tx.clone());
        }
        
        Ok(tx)
    }
    
    /// Complete a transaction
    pub async fn complete_transaction(&self, tx_id: &str, target_tx_hash: &str) -> Result<BridgeTransaction, String> {
        // Validate input parameters
        if tx_id.trim().is_empty() {
            return Err("Transaction ID không được để trống".to_string());
        }
        
        if target_tx_hash.trim().is_empty() {
            return Err("Target transaction hash không được để trống".to_string());
        }
        
        // Check transaction hash format
        if !target_tx_hash.starts_with("0x") || target_tx_hash.len() != 66 {
            return Err(format!("Hash giao dịch không đúng định dạng: {}", target_tx_hash));
        }
        
        // Get the transaction
        let tx = match self.get_transaction(tx_id).await {
            Ok(tx) => tx,
            Err(e) => return Err(format!("Không thể lấy thông tin giao dịch: {}", e)),
        };
        
        // Verify that the transaction is in a valid state to be completed
        if tx.status == BridgeStatus::Completed {
            return Err(format!("Giao dịch đã hoàn thành trước đó"));
        }
        
        if tx.status == BridgeStatus::Failed {
            return Err(format!("Không thể hoàn thành giao dịch đã thất bại"));
        }
        
        // Verify the transaction has gone through the correct states
        let valid_previous_states = vec![
            BridgeStatus::SourceConfirmed,
            BridgeStatus::PendingOnHub,
            BridgeStatus::HubConfirmed,
            BridgeStatus::PendingOnTarget
        ];
        
        if !valid_previous_states.contains(&tx.status) {
            return Err(format!(
                "Giao dịch có trạng thái {:?} không thể chuyển trực tiếp sang Completed", 
                tx.status
            ));
        }
        
        // Find the target adapter
        let target_adapter = self.find_adapter(tx.target_chain)
            .ok_or_else(|| format!("Không tìm thấy adapter cho chain {:?}", tx.target_chain))?;
        
        // Verify the target transaction
        let is_valid = target_adapter.verify_transaction(&tx, target_tx_hash)
            .map_err(|e| format!("Không thể xác minh giao dịch: {}", e))?;
            
        if !is_valid {
            return Err(format!("Giao dịch không hợp lệ"));
        }
        
        // Check if target transaction already exists and is different
        if let Some(existing_hash) = &tx.target_tx_hash {
            if existing_hash != target_tx_hash {
                // Ghi log và từ chối cập nhật hash mới nếu đã có hash khác
                warn!("Có thể bị tấn công thay đổi hash: tx_id={}, hash cũ={}, hash mới={}", 
                    tx_id, existing_hash, target_tx_hash);
                    
                self.log_suspicious_transaction(&tx, &format!(
                    "Cố gắng thay đổi hash: {} -> {}", existing_hash, target_tx_hash
                )).await;
                
                return Err(format!(
                    "Giao dịch đã có target hash khác: {} (khác với hash mới: {})",
                    existing_hash, target_tx_hash
                ));
            }
        }
        
        // Verify transaction amounts using the adapter
        if let Err(e) = target_adapter.verify_transaction_amount(&tx, target_tx_hash).await {
            // Ghi log lỗi và đánh dấu giao dịch đáng ngờ
            warn!("Xác minh số lượng giao dịch thất bại: {}", e);
            self.log_suspicious_transaction(&tx, &format!(
                "Xác minh số lượng thất bại: {}", e
            )).await;
            
            return Err(format!("Xác minh số lượng giao dịch thất bại: {}", e));
        }
        
        // Kiểm tra thêm về tính hợp lệ của giao dịch
        let now = chrono::Utc::now();
        let tx_created = tx.created_at;
        let time_diff_seconds = (now - tx_created).num_seconds();
        
        // Suspicious if completed too quickly or took too long
        if time_diff_seconds < 30 {
            warn!("Giao dịch đáng ngờ: Hoàn thành quá nhanh ({} giây)", time_diff_seconds);
            self.log_suspicious_transaction(&tx, &format!(
                "Hoàn thành quá nhanh: {} giây", time_diff_seconds
            )).await;
        } else if time_diff_seconds > 86400 {
            warn!("Giao dịch đáng ngờ: Mất quá nhiều thời gian để hoàn thành ({} giây)", time_diff_seconds);
            self.log_suspicious_transaction(&tx, &format!(
                "Mất quá nhiều thời gian: {} giây", time_diff_seconds
            )).await;
        }
        
        // Thực hiện kiểm tra bổ sung cho giao dịch lớn
        if let Ok(amount_value) = tx.amount.parse::<f64>() {
            if amount_value > 10000.0 {
                info!("Giao dịch lớn được hoàn thành: {} DMD", amount_value);
                
                // Ghi lại vào logs chi tiết hơn cho giao dịch lớn
                if amount_value > 100000.0 {
                    warn!("Giao dịch rất lớn: {} DMD, ID={}, hash={}", 
                          amount_value, tx_id, target_tx_hash);
                    // Không đánh dấu là đáng ngờ vì giao dịch lớn có thể hợp lệ
                    // nhưng cần được kiểm tra kỹ hơn
                }
            }
        }
        
        // Cập nhật giao dịch
        // Thay vì gọi update_transaction, chúng ta cập nhật trực tiếp và đưa vào hàng đợi
        let mut updated_tx = tx.clone();
        updated_tx.status = BridgeStatus::Completed;
        updated_tx.target_tx_hash = Some(target_tx_hash.to_string());
        updated_tx.updated_at = chrono::Utc::now();
        updated_tx.completed_at = Some(chrono::Utc::now());
        
        // Thêm metadata cho việc hoàn thành
        updated_tx.metadata.insert(
            "completed_time".to_string(),
            chrono::Utc::now().to_rfc3339()
        );
        updated_tx.metadata.insert(
            "time_to_complete_seconds".to_string(),
            time_diff_seconds.to_string()
        );
        
        // Thêm vào hàng đợi cập nhật hàng loạt
        self.queue_transaction_update(updated_tx.clone()).await;
        
        // Cập nhật cache luôn để đảm bảo có kết quả ngay lập tức
        {
            let mut cache = self.tx_cache.write().unwrap();
            cache.put(updated_tx.id.clone(), updated_tx.clone());
        }
        
        // Ghi log thành công
        info!("Giao dịch {} hoàn thành với hash: {}", tx_id, target_tx_hash);
        
        Ok(updated_tx)
    }
    
    /// Thêm giao dịch vào hàng đợi cập nhật hàng loạt
    async fn queue_transaction_update(&self, tx: BridgeTransaction) {
        let mut queue = self.batch_update_queue.lock().await;
        queue.push_back(tx);
        
        // Nếu hàng đợi quá dài, đẩy nhanh việc xử lý
        if queue.len() >= self.max_batch_size {
            // Đặt lại thời gian cập nhật cuối để buộc xử lý sớm
            let mut last_update = self.last_batch_update.write().unwrap();
            *last_update = Instant::now() - Duration::from_millis(self.max_batch_interval_ms);
            
            debug!("Hàng đợi cập nhật đạt ngưỡng ({}), kích hoạt xử lý", queue.len());
        }
    }
    
    /// Update a transaction individually (for urgent cases)
    pub async fn update_transaction_immediate(&self, tx_id: &str, 
                           status: BridgeStatus, 
                           source_tx_hash: Option<String>,
                           target_tx_hash: Option<String>) -> Result<BridgeTransaction, String> {
        // Get the current transaction
        let mut tx = match self.get_transaction(tx_id).await {
            Ok(tx) => tx,
            Err(e) => return Err(format!("Không thể lấy thông tin giao dịch: {}", e)),
        };
        
        // Update the status
        tx.status = status;
        
        // Update the transaction hashes
        if let Some(hash) = source_tx_hash {
            tx.source_tx_hash = Some(hash);
        }
        
        if let Some(hash) = target_tx_hash {
            tx.target_tx_hash = Some(hash);
        }
        
        // Update the timestamps
        tx.updated_at = chrono::Utc::now();
        
        if status == BridgeStatus::Completed || status == BridgeStatus::Failed {
            tx.completed_at = Some(chrono::Utc::now());
        }
        
        // Save the updated transaction
        self.tx_repository.save(&tx)
            .map_err(|e| format!("Không thể lưu giao dịch đã cập nhật: {}", e))?;
        
        // Update the cache
        let mut cache = self.tx_cache.write().unwrap();
        cache.put(tx.id.clone(), tx.clone());
        
        Ok(tx)
    }
    
    /// Log suspicious transaction for later investigation
    async fn log_suspicious_transaction(&self, tx: &BridgeTransaction, reason: &str) -> Result<(), String> {
        // In production, this would log to a secure audit log
        warn!(
            "Giao dịch đáng ngờ: {} - ID: {}, Source: {:?}, Target: {:?}, Amount: {}, Lý do: {}",
            chrono::Utc::now().to_rfc3339(),
            tx.id,
            tx.source_chain,
            tx.target_chain,
            tx.amount,
            reason
        );
        
        // Thêm vào cơ sở dữ liệu audit
        // Trong triển khai thực tế, sẽ lưu chi tiết giao dịch này vào một cơ sở dữ liệu
        // tách biệt để phục vụ điều tra sau này
        
        // Có thể thêm cơ chế thông báo cho người giám sát
        // hoặc đánh giá tự động về giao dịch đáng ngờ
        
        Ok(())
    }
    
    /// Mark a transaction as failed
    pub async fn fail_transaction(&self, tx_id: &str, error_message: &str) -> Result<BridgeTransaction, String> {
        // Validate input parameters
        if tx_id.trim().is_empty() {
            return Err("Transaction ID không được để trống".to_string());
        }
        
        if error_message.trim().is_empty() {
            return Err("Thông báo lỗi không được để trống".to_string());
        }
        
        // Kiểm tra độ dài thông báo lỗi
        if error_message.len() > 1000 {
            return Err("Thông báo lỗi quá dài (tối đa 1000 ký tự)".to_string());
        }
        
        // Get the transaction
        let tx = match self.get_transaction(tx_id).await {
            Ok(tx) => tx,
            Err(e) => return Err(format!("Không thể lấy thông tin giao dịch: {}", e)),
        };
        
        // Verify that the transaction is not already completed or failed
        if tx.status == BridgeStatus::Completed {
            return Err(format!("Không thể đánh dấu thất bại giao dịch đã hoàn thành"));
        }
        
        if tx.status == BridgeStatus::Failed {
            return Err(format!("Giao dịch đã được đánh dấu thất bại trước đó"));
        }
        
        // Ghi log cho các trường hợp đặc biệt
        if tx.status == BridgeStatus::Initiated {
            warn!("Giao dịch được đánh dấu thất bại ở trạng thái khởi tạo (Initiated) - ID: {}", tx_id);
        }
        
        // Kiểm tra thêm có nên từ chối đánh dấu thất bại trong một số trường hợp
        if tx.status == BridgeStatus::HubConfirmed || tx.status == BridgeStatus::SourceConfirmed {
            warn!("Giao dịch đã được xác nhận nhưng đang được đánh dấu thất bại - ID: {}, Status: {:?}", 
                  tx_id, tx.status);
            
            // Ghi log chi tiết là đáng ngờ vì giao dịch đã được xác nhận nhưng đang thất bại
            self.log_suspicious_transaction(&tx, &format!(
                "Đánh dấu thất bại từ trạng thái đã xác nhận: {:?}", tx.status
            )).await;
        }
        
        // Sanitize error message - filter out potential code injection or HTML
        let sanitized_error = error_message
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#39;")
            .replace(';', "&#59;")
            .replace('(', "&#40;")
            .replace(')', "&#41;");
            
        // Check if this transaction has failed too many times (in metadata)
        let failure_attempts = tx.metadata.get("failure_attempts")
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
            
        if failure_attempts >= 3 {
            warn!("Giao dịch {} đã thất bại {} lần, có thể cần kiểm tra thủ công", tx_id, failure_attempts);
            
            // Nếu số lần thất bại quá nhiều, đánh dấu là đáng ngờ
            if failure_attempts >= 5 {
                self.log_suspicious_transaction(&tx, &format!(
                    "Đã thất bại quá nhiều lần: {}", failure_attempts
                )).await;
            }
        }
        
        // Get time difference to ensure transaction is not suspicious
        let now = chrono::Utc::now();
        let tx_created = tx.created_at;
        let time_diff_seconds = (now - tx_created).num_seconds();
        
        // Suspicious if failed too quickly
        if time_diff_seconds < 10 {
            warn!("Giao dịch đáng ngờ: Thất bại quá nhanh ({} giây)", time_diff_seconds);
            self.log_suspicious_transaction(&tx, &format!(
                "Thất bại quá nhanh: {} giây", time_diff_seconds
            )).await;
        }
        
        // Kiểm tra nội dung lỗi có đáng ngờ không
        let suspicious_error_terms = vec![
            "hack", "exploit", "attack", "bypass", "overflow", "underflow",
            "steal", "theft", "unauthorized", "compromised"
        ];
        
        for term in suspicious_error_terms {
            if error_message.to_lowercase().contains(term) {
                warn!("Thông báo lỗi có chứa thuật ngữ đáng ngờ: {}", term);
                self.log_suspicious_transaction(&tx, &format!(
                    "Thông báo lỗi có chứa thuật ngữ đáng ngờ: {}", term
                )).await;
                break;
            }
        }
        
        // Update the transaction
        let mut updated_tx = tx.clone();
        updated_tx.status = BridgeStatus::Failed;
        updated_tx.error_message = Some(sanitized_error);
        updated_tx.updated_at = chrono::Utc::now();
        updated_tx.completed_at = Some(chrono::Utc::now());
        
        // Update metadata
        updated_tx.metadata.insert(
            "failure_attempts".to_string(),
            (failure_attempts + 1).to_string()
        );
        updated_tx.metadata.insert(
            "last_failure_time".to_string(),
            chrono::Utc::now().to_rfc3339()
        );
        updated_tx.metadata.insert(
            "failure_message".to_string(),
            sanitized_error.clone()
        );
        
        // Log the failure
        error!(
            "Giao dịch {} thất bại: {} - Source: {:?}, Target: {:?}, Amount: {}",
            tx_id, sanitized_error, tx.source_chain, tx.target_chain, tx.amount
        );
        
        // Save the updated transaction
        self.tx_repository.save(&updated_tx)
            .map_err(|e| format!("Không thể lưu giao dịch đã cập nhật: {}", e))?;
        
        // Update the cache
        let mut cache = self.tx_cache.write().unwrap();
        cache.put(updated_tx.id.clone(), updated_tx.clone());
        
        Ok(updated_tx)
    }
    
    /// Get a bridge transaction
    pub async fn get_transaction(&self, tx_id: &str) -> Result<BridgeTransaction, String> {
        // First check the cache
        {
            let mut cache = self.tx_cache.write().unwrap();
            if let Some(tx) = cache.get(tx_id) {
                return Ok(tx.clone());
            }
        }
        
        // Not found in cache, check the repository
        match self.tx_repository.find_by_id(tx_id) {
            Ok(Some(tx)) => {
                // Update the cache
                let mut cache = self.tx_cache.write().unwrap();
                cache.put(tx.id.clone(), tx.clone());
                Ok(tx)
            },
            Ok(None) => Err(format!("Không tìm thấy giao dịch với ID: {}", tx_id)),
            Err(e) => Err(format!("Lỗi khi tìm kiếm giao dịch: {}", e)),
        }
    }
    
    /// Get transactions by source address
    pub async fn get_transactions_by_source_address(&self, address: &str) -> Result<Vec<BridgeTransaction>, String> {
        self.tx_repository.find_by_source_address(address)
    }
    
    /// Get transactions by target address
    pub async fn get_transactions_by_target_address(&self, address: &str) -> Result<Vec<BridgeTransaction>, String> {
        self.tx_repository.find_by_target_address(address)
    }
    
    /// Get transactions by status
    pub async fn get_transactions_by_status(&self, status: BridgeStatus) -> Result<Vec<BridgeTransaction>, String> {
        self.tx_repository.find_by_status(&status)
    }
    
    /// Clean up old transactions
    pub async fn cleanup_old_transactions(&self, older_than_days: u64) -> Result<usize, String> {
        // Calculate the cutoff timestamp
        let now = chrono::Utc::now();
        let cutoff = now.timestamp() as u64 - (older_than_days * 24 * 60 * 60);
        
        // Delete transactions older than the cutoff
        self.tx_repository.delete_older_than(cutoff)
    }
    
    /// Get all transactions
    pub async fn get_all_transactions(&self) -> Result<Vec<BridgeTransaction>, String> {
        self.tx_repository.get_all_transactions()
    }

    /// Khởi tạo bridge
    pub async fn initiate_bridge(
        &self,
        source_chain: &str,
        target_chain: &str,
        source_address: &str,
        target_address: &str,
        amount: &str,
    ) -> BridgeResult<BridgeTransaction> {
        // Xác thực dữ liệu đầu vào
        self.validate_bridge_params(source_chain, target_chain, source_address, target_address, amount).await?;
        
        // Kiểm tra chuỗi nguồn và đích có được hỗ trợ không
        if !self.is_supported_chain(source_chain) {
            return Err(BridgeError::UnsupportedChain(format!("Chuỗi nguồn không được hỗ trợ: {}", source_chain)));
        }

        if !self.is_supported_chain(target_chain) {
            return Err(BridgeError::UnsupportedChain(format!("Chuỗi đích không được hỗ trợ: {}", target_chain)));
        }
        
        // Xác thực số lượng
        let amount_value = match amount.parse::<f64>() {
            Ok(val) => {
                if val <= 0.0 {
                    return Err(BridgeError::InvalidAmount(format!("Số lượng phải lớn hơn 0: {}", amount)));
                }
                val
            },
            Err(_) => return Err(BridgeError::InvalidAmount(format!("Số lượng không hợp lệ: {}", amount))),
        };
        
        // Tính phí giao dịch
        let fee = self.calculate_fee(source_chain, target_chain, amount).await?;
        
        // Kiểm tra số dư
        self.check_balance(source_chain, source_address, &(amount_value + fee).to_string()).await?;
        
        // Tạo giao dịch bridge
        let transaction = BridgeTransaction {
            id: Some(Uuid::new_v4().to_string()),
            source_chain: source_chain.to_string(),
            target_chain: target_chain.to_string(),
            source_address: source_address.to_string(),
            target_address: target_address.to_string(),
            amount: amount.to_string(),
            fee: fee.to_string(),
            status: BridgeTransactionStatus::Initiated,
            source_tx_hash: None,
            target_tx_hash: None,
            created_at: SystemTime::now(),
            updated_at: SystemTime::now(),
        };
        
        // Tạo giao dịch trong kho lưu trữ
        if let Some(repo) = &self.transaction_repository {
            repo.save(&transaction).await?;
        }
        
        // Trả về giao dịch đã tạo
        Ok(transaction)
    }
    
    /// Xác thực các tham số bridge
    async fn validate_bridge_params(
        &self, 
        source_chain: &str, 
        target_chain: &str, 
        source_address: &str, 
        target_address: &str, 
        amount: &str
    ) -> BridgeResult<()> {
        // Kiểm tra chuỗi nguồn và đích khác nhau
        if source_chain == target_chain {
            return Err(BridgeError::ValidationError(
                "Chuỗi nguồn và chuỗi đích phải khác nhau".into()
            ));
        }
        
        // Kiểm tra định dạng địa chỉ
        self.validate_address(source_address, source_chain)?;
        self.validate_address(target_address, target_chain)?;
        
        // Kiểm tra số lượng
        let amount_value = match amount.parse::<f64>() {
            Ok(v) => v,
            Err(_) => return Err(BridgeError::InvalidAmount(amount.to_string())),
        };
        
        if amount_value <= 0.0 {
            return Err(BridgeError::InvalidAmount(
                "Số lượng token phải lớn hơn 0".into()
            ));
        }
        
        // Kiểm tra giới hạn số lượng
        self.validate_amount_limits(source_chain, target_chain, amount_value).await?;
        
        Ok(())
    }
    
    /// Kiểm tra định dạng địa chỉ
    fn validate_address(&self, address: &str, chain: &str) -> BridgeResult<()> {
        match chain.to_lowercase().as_str() {
            "ethereum" | "bsc" | "polygon" | "aurora" => {
                // Kiểm tra định dạng địa chỉ ETH (0x...)
                if !address.starts_with("0x") || address.len() != 42 {
                    return Err(BridgeError::InvalidAddress(format!(
                        "Địa chỉ {} không hợp lệ cho chuỗi {}", address, chain
                    )));
                }
            },
            "near" => {
                // Kiểm tra định dạng địa chỉ NEAR
                if address.len() < 2 || address.len() > 64 || !address.contains('.') {
                    return Err(BridgeError::InvalidAddress(format!(
                        "Địa chỉ {} không hợp lệ cho chuỗi {}", address, chain
                    )));
                }
            },
            _ => {
                // Đối với các chuỗi khác, thực hiện kiểm tra chung
                if address.len() < 10 {
                    return Err(BridgeError::InvalidAddress(format!(
                        "Địa chỉ {} không hợp lệ", address
                    )));
                }
            }
        }
        
        Ok(())
    }
    
    /// Kiểm tra giới hạn số lượng
    async fn validate_amount_limits(&self, source_chain: &str, target_chain: &str, amount: f64) -> BridgeResult<()> {
        // Lấy giới hạn cho cặp chuỗi
        let adapter = self.find_adapter(source_chain, target_chain)?;
        let limits = adapter.get_chain_limits(source_chain, target_chain)?;
        
        if amount < limits.0 {
            return Err(BridgeError::InvalidAmount(format!(
                "Số lượng ({}) nhỏ hơn giới hạn tối thiểu ({}) cho cặp chuỗi {}-{}",
                amount, limits.0, source_chain, target_chain
            )));
        }
        
        if amount > limits.1 {
            return Err(BridgeError::InvalidAmount(format!(
                "Số lượng ({}) lớn hơn giới hạn tối đa ({}) cho cặp chuỗi {}-{}",
                amount, limits.1, source_chain, target_chain
            )));
        }
        
        Ok(())
    }
    
    /// Kiểm tra số dư của người dùng
    async fn check_balance(&self, chain: &str, address: &str, amount: &str) -> BridgeResult<()> {
        // Lấy provider cho chuỗi
        let provider = self.get_provider(chain)?;
        
        // Chuyển đổi số lượng thành số
        let amount_value = match amount.parse::<f64>() {
            Ok(v) => v,
            Err(_) => return Err(BridgeError::InvalidAmount(amount.to_string())),
        };
        
        // Lấy số dư của tài khoản
        let balance = provider.get_balance(address).await?;
        
        // Chuyển đổi số dư thành số
        let balance_value = match balance.parse::<f64>() {
            Ok(v) => v,
            Err(_) => return Err(BridgeError::ProviderError("Không thể đọc số dư".into())),
        };
        
        // Kiểm tra số dư đủ không
        if balance_value < amount_value {
            return Err(BridgeError::InsufficientBalance(format!(
                "Số dư ({}) không đủ để thực hiện giao dịch với số lượng {}",
                balance_value, amount_value
            )));
        }
        
        Ok(())
    }

    /// Verify that the sequence of transactions from a given wallet address is valid
    /// This helps detect suspicious patterns like:
    /// - Multiple small transactions following large deposits
    /// - Circular transactions between wallets
    /// - Rapid sequence of transactions between the same wallets
    pub async fn verify_transaction_sequence(&self, wallet_address: &str, chain: Chain) -> Result<bool, String> {
        // Kiểm tra định dạng địa chỉ
        if !self.is_valid_address(wallet_address, chain) {
            return Err(format!("Địa chỉ ví không hợp lệ cho blockchain {}: {}", chain, wallet_address));
        }

        // Lấy các giao dịch gần đây của ví
        let wallet_transactions = match self.get_wallet_transactions(wallet_address, chain) {
            Ok(txs) => txs,
            Err(e) => return Err(format!("Không thể lấy lịch sử giao dịch: {}", e)),
        };
        
        if wallet_transactions.is_empty() {
            // Không có giao dịch nào, không thể xác minh
            return Ok(true);
        }
        
        // Check for suspicious patterns
        
        // 1. Nhiều giao dịch nhỏ sau khi có giao dịch lớn (có thể là chia nhỏ để tránh phát hiện)
        let large_deposit_threshold = 10000.0; // Ngưỡng coi là giao dịch lớn
        let small_tx_threshold = 1000.0;       // Ngưỡng coi là giao dịch nhỏ
        let suspicious_small_tx_count = 5;     // Số lượng giao dịch nhỏ coi là đáng ngờ
        
        // Sắp xếp giao dịch theo thời gian
        let mut sorted_txs = wallet_transactions.clone();
        sorted_txs.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        
        // Kiểm tra giao dịch lớn vào và nhiều giao dịch nhỏ ra
        let mut found_large_deposit = false;
        let mut small_tx_after_deposit = 0;
        let mut large_deposit_time = None;
        
        for tx in &sorted_txs {
            if let Ok(amount) = tx.amount.parse::<f64>() {
                // Nếu là giao dịch đến ví này và là giao dịch lớn
                if tx.target_address == wallet_address && amount >= large_deposit_threshold {
                    found_large_deposit = true;
                    large_deposit_time = Some(tx.created_at);
                } 
                // Nếu là giao dịch từ ví này và là giao dịch nhỏ sau khi có giao dịch lớn vào
                else if found_large_deposit && tx.source_address == wallet_address && 
                         amount <= small_tx_threshold {
                    // Kiểm tra thêm thời gian
                    if let Some(deposit_time) = large_deposit_time {
                        if (tx.created_at - deposit_time).num_hours() < 24 {
                            small_tx_after_deposit += 1;
                        }
                    }
                }
            }
        }
        
        if found_large_deposit && small_tx_after_deposit >= suspicious_small_tx_count {
            warn!(
                "Phát hiện mẫu đáng ngờ: {} giao dịch nhỏ sau một giao dịch lớn vào ví {} trên {:?}", 
                small_tx_after_deposit, wallet_address, chain
            );
            
            // Ghi log mẫu đáng ngờ này
            for tx in &sorted_txs {
                if let Ok(amount) = tx.amount.parse::<f64>() {
                    if tx.source_address == wallet_address && amount <= small_tx_threshold {
                        self.log_suspicious_transaction(tx, "Giao dịch nhỏ sau khi nhận giao dịch lớn").await;
                    }
                }
            }
            
            return Ok(false);
        }
        
        // 2. Giao dịch vòng tròn giữa các ví
        let cycle_detection_limit = 10; // Số lượng giao dịch để kiểm tra chu kỳ
        if sorted_txs.len() >= cycle_detection_limit {
            let recent_txs = &sorted_txs[sorted_txs.len() - cycle_detection_limit..];
            
            // Tạo đồ thị các giao dịch
            let mut tx_graph: HashMap<String, Vec<String>> = HashMap::new();
            
            for tx in recent_txs {
                let source = tx.source_address.clone();
                let target = tx.target_address.clone();
                
                tx_graph.entry(source.clone())
                    .or_insert_with(Vec::new)
                    .push(target.clone());
            }
            
            // Kiểm tra chu kỳ đơn giản (A->B->C->A)
            if self.detect_simple_cycle(wallet_address, &tx_graph) {
                warn!(
                    "Phát hiện giao dịch vòng tròn liên quan đến ví {} trên {:?}", 
                    wallet_address, chain
                );
                
                // Ghi log các giao dịch trong chu kỳ
                for tx in recent_txs {
                    self.log_suspicious_transaction(tx, "Giao dịch vòng tròn tiềm ẩn").await;
                }
                
                return Ok(false);
            }
        }
        
        // 3. Kiểm tra tần suất giao dịch
        let rapid_tx_timeframe = 1; // Số giờ
        let rapid_tx_threshold = 10; // Số lượng giao dịch trong khung giờ để coi là đáng ngờ
        
        // Đếm số giao dịch trong mỗi khung giờ
        let mut tx_count_per_hour: HashMap<i64, usize> = HashMap::new();
        
        for tx in &sorted_txs {
            let hour_bucket = tx.created_at.timestamp() / 3600;
            *tx_count_per_hour.entry(hour_bucket).or_insert(0) += 1;
        }
        
        // Kiểm tra xem có khung giờ nào vượt ngưỡng không
        let max_tx_per_hour = tx_count_per_hour.values().cloned().max().unwrap_or(0);
        
        if max_tx_per_hour >= rapid_tx_threshold {
            warn!(
                "Phát hiện tần suất giao dịch cao: {} giao dịch trong 1 giờ từ ví {} trên {:?}", 
                max_tx_per_hour, wallet_address, chain
            );
            
            // Ghi log những giao dịch trong khung giờ cao điểm
            let peak_hour = tx_count_per_hour
                .iter()
                .max_by_key(|(_, &count)| count)
                .map(|(&hour, _)| hour)
                .unwrap_or(0);
                
            for tx in &sorted_txs {
                let tx_hour = tx.created_at.timestamp() / 3600;
                if tx_hour == peak_hour {
                    self.log_suspicious_transaction(tx, "Giao dịch trong khung giờ có tần suất cao").await;
                }
            }
            
            return Ok(false);
        }
        
        // Giao dịch đã vượt qua tất cả kiểm tra
        Ok(true)
    }
    
    /// Hàm hỗ trợ kiểm tra chu kỳ đơn giản trong đồ thị giao dịch
    fn detect_simple_cycle(&self, start_node: &str, graph: &HashMap<String, Vec<String>>) -> bool {
        // Dùng thuật toán DFS để phát hiện chu kỳ
        let mut visited: HashSet<String> = HashSet::new();
        let mut path: HashSet<String> = HashSet::new();
        
        fn dfs(
            current: &str, 
            target: &str,
            graph: &HashMap<String, Vec<String>>,
            visited: &mut HashSet<String>,
            path: &mut HashSet<String>,
            depth: usize
        ) -> bool {
            // Thêm nút hiện tại vào đường đi
            path.insert(current.to_string());
            
            // Nếu đã quá sâu, dừng tìm kiếm để tránh loop quá lâu
            if depth > 10 {
                path.remove(current);
                return false;
            }
            
            // Nếu có cạnh từ nút hiện tại đến đích, đã tìm thấy chu kỳ
            if let Some(neighbors) = graph.get(current) {
                if neighbors.contains(&target.to_string()) && depth >= 2 {
                    // Chu kỳ phải có ít nhất 3 nút (depth >= 2)
                    return true;
                }
                
                // Duyệt qua các nút kề
                for next in neighbors {
                    // Tránh xử lý lại các nút đã duyệt qua
                    if !visited.contains(next) {
                        visited.insert(next.to_string());
                        
                        if !path.contains(next) {
                            if dfs(next, target, graph, visited, path, depth + 1) {
                                return true;
                            }
                        }
                    }
                }
            }
            
            // Không tìm thấy chu kỳ từ nút hiện tại
            path.remove(current);
            false
        }
        
        // Bắt đầu tìm kiếm từ nút xuất phát
        dfs(start_node, start_node, graph, &mut visited, &mut path, 0)
    }
    
    /// Lấy danh sách các giao dịch liên quan đến ví cụ thể
    async fn get_wallet_transactions(&self, wallet_address: &str, chain: Chain) 
        -> Result<Vec<BridgeTransaction>, String> {
        // Lấy tất cả giao dịch
        let all_transactions = self.tx_repository.get_all()
            .map_err(|e| format!("Không thể lấy danh sách giao dịch: {}", e))?;
            
        // Lọc các giao dịch liên quan đến ví và chain cụ thể
        let mut wallet_transactions = Vec::new();
        
        for tx in all_transactions {
            // Chỉ quan tâm đến giao dịch trên chain cụ thể
            if tx.source_chain == chain || tx.target_chain == chain {
                // Chỉ quan tâm đến giao dịch liên quan đến ví này
                if tx.source_address == wallet_address || tx.target_address == wallet_address {
                    wallet_transactions.push(tx);
                }
            }
        }
        
        // Chỉ quan tâm đến giao dịch gần đây (1 tuần)
        let one_week_ago = chrono::Utc::now() - chrono::Duration::days(7);
        
        wallet_transactions.retain(|tx| tx.created_at > one_week_ago);
        
        Ok(wallet_transactions)
    }

    /// Xác thực định dạng địa chỉ ví dựa trên loại blockchain
    fn is_valid_address(&self, address: &str, chain: Chain) -> bool {
        if address.trim().is_empty() {
            return false;
        }
        
        match chain {
            Chain::Ethereum | Chain::BSC | Chain::Polygon | Chain::Arbitrum => {
                // Địa chỉ EVM dạng 0x + 40 ký tự hex
                let eth_regex = Regex::new(r"^0x[a-fA-F0-9]{40}$").unwrap();
                eth_regex.is_match(address)
            },
            Chain::Near => {
                // Địa chỉ NEAR thường kết thúc bằng .near hoặc là 64 ký tự base58
                let near_account_regex = Regex::new(r"^[a-z0-9_-]{1,64}\.(testnet|near)$").unwrap();
                let near_implicit_regex = Regex::new(r"^[a-zA-Z0-9]{64}$").unwrap();
                near_account_regex.is_match(address) || near_implicit_regex.is_match(address)
            },
            Chain::Solana => {
                // Địa chỉ Solana là 32-44 ký tự base58
                let sol_regex = Regex::new(r"^[1-9A-HJ-NP-Za-km-z]{32,44}$").unwrap();
                sol_regex.is_match(address)
            },
            Chain::Bitcoin => {
                // Địa chỉ Bitcoin bắt đầu bằng 1, 3, bc1
                let btc_legacy_regex = Regex::new(r"^[13][a-km-zA-HJ-NP-Z1-9]{25,34}$").unwrap();
                let btc_segwit_regex = Regex::new(r"^bc1[a-zA-HJ-NP-Z0-9]{39,59}$").unwrap();
                btc_legacy_regex.is_match(address) || btc_segwit_regex.is_match(address)
            },
            Chain::Aptos => {
                // Địa chỉ Aptos là 0x + 64 ký tự hex
                let aptos_regex = Regex::new(r"^0x[a-fA-F0-9]{64}$").unwrap();
                aptos_regex.is_match(address)
            },
            _ => {
                // Đối với các chain khác, kiểm tra chung
                // Ít nhất 5 ký tự và không chứa ký tự đặc biệt ngoại trừ '.', '-', và '_'
                let general_regex = Regex::new(r"^[a-zA-Z0-9\.\-_]{5,}$").unwrap();
                general_regex.is_match(address)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockall::predicate::*;
    use mockall::*;
    
    mock! {
        BlockchainProviderMock {}
        
        impl Clone for BlockchainProviderMock {
            fn clone(&self) -> Self;
        }
        
        impl BlockchainProvider for BlockchainProviderMock {}
    }
    
    #[test]
    fn test_is_evm_chain() {
        assert!(is_evm_chain(&DmdChain::Ethereum));
        assert!(is_evm_chain(&DmdChain::BinanceSmartChain));
        assert!(!is_evm_chain(&DmdChain::Near));
        assert!(!is_evm_chain(&DmdChain::Solana));
    }
    
    #[tokio::test]
    async fn test_bridge_manager_create_transaction() {
        let mock_provider = Arc::new(MockBlockchainProviderMock::new());
        let bridge_manager = BridgeManager::new(mock_provider);
        
        let result = bridge_manager.create_transaction(
            DmdChain::Ethereum,
            DmdChain::Near,
            "0x1234...".to_string(),
            "near.testnet".to_string(),
            1000,
        );
        
        assert!(result.is_ok());
        let tx_id = result.unwrap();
        
        // Kiểm tra giao dịch đã được tạo
        let tx = bridge_manager.get_transaction(&tx_id).unwrap();
        assert_eq!(tx.source_chain, DmdChain::Ethereum);
        assert_eq!(tx.target_chain, DmdChain::Near);
        assert_eq!(tx.amount, 1000);
        assert_eq!(tx.status, BridgeTransactionStatus::Created);
    }
    
    #[tokio::test]
    async fn test_bridge_manager_unsupported_route() {
        let mock_provider = Arc::new(MockBlockchainProviderMock::new());
        let bridge_manager = BridgeManager::new(mock_provider);
        
        let result = bridge_manager.create_transaction(
            DmdChain::Solana,
            DmdChain::Ethereum,
            "solana_address".to_string(),
            "0x1234...".to_string(),
            1000,
        );
        
        assert!(result.is_err());
        match result {
            Err(BridgeError::UnsupportedRoute(_)) => {}
            _ => panic!("Expected UnsupportedRoute error"),
        }
    }
} 