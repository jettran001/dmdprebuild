#[cfg(feature = "ethereum")]
use ethers::prelude::*;
#[cfg(feature = "near")]
use near_jsonrpc_client::JsonRpcClient;
#[cfg(feature = "solana")]
use solana_client::rpc_client::RpcClient;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::error::Error;
use hex;
use chrono;
use serde_json;
use sqlx;
use std::time::Duration;
use log::{debug, info, warn, error};
use std::collections::HashMap;
use async_trait::async_trait;
use sqlx::PgPool;
use serde::{Serialize, Deserialize};
use futures::Future;
use futures::future::BoxFuture;
use metrics;

// Định nghĩa các trạng thái bridge
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BridgeStatus {
    Pending,
    Confirmed,
    Failed(String),
    Completed,
}

// Định nghĩa các chuỗi hỗ trợ
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Chain {
    BSC,
    NEAR,
    Solana,
    Ethereum,
    Polygon,
    Avalanche,
}

impl Chain {
    /// Chuyển đổi chain sang LayerZero chain ID
    pub fn to_layerzero_id(&self) -> u16 {
        match self {
            Chain::BSC => 2,         // BSC trong LayerZero
            Chain::NEAR => 115,      // NEAR trong LayerZero
            Chain::Ethereum => 1,    // Ethereum trong LayerZero
            Chain::Polygon => 4,     // Polygon trong LayerZero
            Chain::Avalanche => 3,   // Avalanche trong LayerZero
            Chain::Solana => 0,      // Solana không dùng LayerZero mà dùng Wormhole
        }
    }
    
    /// Chuyển đổi chain sang string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            Chain::BSC => "bsc",
            Chain::NEAR => "near",
            Chain::Solana => "solana",
            Chain::Ethereum => "ethereum",
            Chain::Polygon => "polygon",
            Chain::Avalanche => "avalanche",
        }
    }
    
    /// Chuyển đổi từ LayerZero chain ID sang Chain enum
    pub fn from_layerzero_id(id: u16) -> Option<Self> {
        match id {
            1 => Some(Chain::Ethereum),
            2 => Some(Chain::BSC),
            3 => Some(Chain::Avalanche),
            4 => Some(Chain::Polygon),
            115 => Some(Chain::NEAR),
            _ => None,
        }
    }
    
    /// Kiểm tra xem chain có được supported bởi LayerZero không
    pub fn is_layerzero_supported(&self) -> bool {
        match self {
            Chain::Solana => false,  // Solana không hỗ trợ LayerZero
            _ => true,              // Các chain khác đều hỗ trợ
        }
    }
    
    /// Kiểm tra xem chain có được supported bởi Wormhole không
    pub fn is_wormhole_supported(&self) -> bool {
        match self {
            Chain::NEAR => false,    // NEAR không hỗ trợ Wormhole
            _ => true,              // Các chain khác đều hỗ trợ
        }
    }
    
    /// Danh sách các chain được hỗ trợ cho bridge
    pub fn supported_chains() -> Vec<Self> {
        vec![
            Self::BSC,
            Self::NEAR,
            Self::Solana,
            Self::Ethereum,
            Self::Polygon,
            Self::Avalanche
        ]
    }
}

impl ToString for Chain {
    fn to_string(&self) -> String {
        self.as_str().to_string()
    }
}

// Struct chứa thông tin ước tính phí
#[derive(Debug, Clone, Serialize)]
pub struct FeeEstimate {
    pub fee_amount: String,  // Số lượng phí theo số nguyên
    pub fee_token: String,   // Token của phí (VD: "BNB", "ETH", ...)
    pub fee_usd: f64,        // Quy đổi sang USD
}

// Interface cho LayerZero client
#[async_trait]
pub trait LayerZeroClient: Send + Sync + 'static {
    async fn send_message(&self, from_chain_id: u16, to_chain_id: u16, receiver: String, payload: Vec<u8>) -> Result<String, Box<dyn Error>>;
    async fn get_transaction_status(&self, tx_hash: &str) -> Result<String, Box<dyn Error>>;
    async fn estimate_fee(&self, from_chain_id: u16, to_chain_id: u16, payload_size: usize) -> Result<FeeEstimate, Box<dyn Error>>;
}

// Interface cho Wormhole client
#[async_trait]
pub trait WormholeClient: Send + Sync + 'static {
    async fn send_message(&self, from_chain: &str, to_chain: &str, receiver: String, payload: Vec<u8>) -> Result<String, Box<dyn Error>>;
    async fn get_transaction_status(&self, tx_hash: &str) -> Result<String, Box<dyn Error>>;
    async fn estimate_fee(&self, from_chain: &str, to_chain: &str, payload_size: usize) -> Result<FeeEstimate, Box<dyn Error>>;
}

// Thông tin về một giao dịch bridge
#[derive(Debug, Clone)]
pub struct BridgeTransaction {
    tx_hash: String,
    source_chain: Chain,
    target_chain: Chain,
    sender: String,
    receiver: String,
    amount: U256,
    token_id: u64,
    status: BridgeStatus,
    timestamp: u64,
}

// Orchestrator chính
pub struct BridgeOrchestrator {
    bsc_provider: Provider<Ws>,
    near_client: JsonRpcClient,
    solana_client: RpcClient,
    bridge_contract_bsc: Address,
    token_contract_bsc: Address,
    // Các client cho LayerZero và Wormhole
    layerzero_client: Arc<dyn LayerZeroClient>,
    wormhole_client: Arc<dyn WormholeClient>,
    // Cache và DB
    transactions: Arc<RwLock<HashMap<String, BridgeTransaction>>>,
    db_pool: PgPool,
}

// Mapping sự kiện từ các contract
#[derive(Debug, Clone)]
pub struct TokenBridgedEvent {
    pub tx_hash: H256,
    pub from: Address,
    pub id: U256,
    pub amount: U256,
    pub to_chain_id: u16,
    pub to_address: Vec<u8>,
}

// Khai báo filter cho event TokenBridged
#[derive(Debug, Clone)]
pub struct TokenBridgedFilter;

/// Cấu hình cho quá trình monitoring giao dịch bridge
#[derive(Clone, Debug)]
pub struct MonitorConfig {
    /// Số lần retry tối đa
    pub max_retries: u32,
    /// Thời gian chờ ban đầu giữa các lần retry (giây)
    pub initial_delay: u64,
    /// Hệ số tăng thời gian chờ theo cấp số nhân
    pub backoff_factor: f32,
    /// Thời gian tối đa để chờ một giao dịch hoàn thành (giây)
    pub max_timeout: u64,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            max_retries: 5,
            initial_delay: 30,
            backoff_factor: 1.5,
            max_timeout: 3600, // 1 giờ
        }
    }
}

impl BridgeOrchestrator {
    // constructor
    pub async fn new(
        bsc_ws_url: &str,
        near_rpc_url: &str,
        solana_rpc_url: &str,
        bridge_contract_bsc: Address,
        token_contract_bsc: Address,
        layerzero_client: Arc<dyn LayerZeroClient>,
        wormhole_client: Arc<dyn WormholeClient>,
        db_pool: PgPool,
    ) -> Result<Self, Box<dyn Error>> {
        let bsc_provider = Provider::<Ws>::connect(bsc_ws_url).await?;
        let near_client = JsonRpcClient::connect(near_rpc_url);
        let solana_client = RpcClient::new(solana_rpc_url.to_string());
        
        Ok(Self {
            bsc_provider,
            near_client,
            solana_client,
            bridge_contract_bsc,
            token_contract_bsc,
            layerzero_client,
            wormhole_client,
            transactions: Arc::new(RwLock::new(HashMap::new())),
            db_pool,
        })
    }

    // Lắng nghe events từ BSC bridge contract
    pub async fn listen_bsc_events(&self) -> Result<(), Box<dyn Error>> {
        let contract = Contract::new(
            self.bridge_contract_bsc,
            include_bytes!("../abi/DmdBscBridge.json"),
            self.bsc_provider.clone(),
        );
        
        let filter = contract.event::<TokenBridgedFilter>().from_block(BlockNumber::Latest);
        let mut stream = filter.subscribe().await?;
        
        while let Some(event) = stream.next().await {
            if let Ok(log) = event {
                // Xử lý event TokenBridged
                self.process_token_bridged_event(log).await?;
            }
        }
        
        Ok(())
    }
    
    // Xử lý event TokenBridged
    async fn process_token_bridged_event(&self, event: TokenBridgedEvent) -> Result<(), Box<dyn Error>> {
        let tx = BridgeTransaction {
            tx_hash: event.tx_hash.to_string(),
            source_chain: Chain::BSC,
            target_chain: self.determine_target_chain(event.to_chain_id)?,
            sender: event.from.to_string(),
            receiver: hex::encode(event.to_address.clone()),
            amount: event.amount,
            token_id: event.id.as_u64(),
            status: BridgeStatus::Pending,
            timestamp: chrono::Utc::now().timestamp() as u64,
        };
        
        // Lưu transaction vào cache và DB
        self.store_transaction(&tx).await?;
        
        // Bắt đầu xác thực và relay
        self.validate_and_relay(tx).await
    }
    
    // Xác thực payload và relay sang chain đích
    async fn validate_and_relay(&self, tx: BridgeTransaction) -> Result<(), Box<dyn Error>> {
        // Xác thực các thông tin từ transaction
        if !self.validate_transaction(&tx).await? {
            self.update_transaction_status(&tx.tx_hash, BridgeStatus::Failed("Validation failed".into())).await?;
            return Err("Transaction validation failed".into());
        }
        
        // Relay dựa trên target chain
        match tx.target_chain {
            Chain::NEAR => self.relay_to_near(&tx).await?,
            Chain::Solana => self.relay_to_solana(&tx).await?,
            _ => return Err("Unsupported target chain".into()),
        }
        
        // Cập nhật trạng thái
        self.update_transaction_status(&tx.tx_hash, BridgeStatus::Confirmed).await?;
        
        Ok(())
    }
    
    // Relay tới NEAR thông qua LayerZero
    async fn relay_to_near(&self, tx: &BridgeTransaction) -> Result<(), Box<dyn Error>> {
        // Chuẩn bị payload cho LayerZero
        let payload = self.prepare_layerzero_payload(tx)?;
        
        // Gửi payload qua LayerZero
        let lz_tx = self.layerzero_client.send_message(
            Chain::BSC.to_layerzero_id(),
            Chain::NEAR.to_layerzero_id(), 
            tx.receiver.clone(), 
            payload
        ).await?;
        
        // Theo dõi trạng thái của giao dịch LayerZero
        tokio::spawn(self.clone().monitor_transaction(
            tx.tx_hash.clone(),
            lz_tx,
            None,
            |hash| {
                let client = self.layerzero_client.clone();
                Box::pin(async move {
                    client.get_transaction_status(hash).await
                })
            },
            |tx| self.relay_to_near(tx)
        ));
        
        Ok(())
    }
    
    // Relay tới Solana thông qua Wormhole
    async fn relay_to_solana(&self, tx: &BridgeTransaction) -> Result<(), Box<dyn Error>> {
        // Chuẩn bị payload cho Wormhole
        let payload = self.prepare_wormhole_payload(tx).await?;
        
        // Gửi payload qua Wormhole
        let wh_tx = self.wormhole_client.send_message(
            Chain::BSC.as_str(),
            Chain::Solana.as_str(),
            tx.receiver.clone(),
            payload
        ).await?;
        
        // Theo dõi trạng thái của giao dịch Wormhole
        tokio::spawn(self.clone().monitor_transaction(
            tx.tx_hash.clone(),
            wh_tx,
            None,
            |hash| {
                let client = self.wormhole_client.clone();
                Box::pin(async move {
                    client.get_transaction_status(hash).await
                })
            },
            |tx| self.relay_to_solana(tx)
        ));
        
        Ok(())
    }
    
    // Lưu trạng thái của giao dịch
    async fn store_transaction(&self, tx: &BridgeTransaction) -> Result<(), Box<dyn Error>> {
        // Lưu vào cache
        let mut transactions = self.transactions.write().await;
        transactions.insert(tx.tx_hash.clone(), tx.clone());
        
        // Lưu vào database
        sqlx::query!(
            "INSERT INTO bridge_transactions 
            (tx_hash, source_chain, target_chain, sender, receiver, amount, token_id, status, timestamp) 
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            tx.tx_hash,
            tx.source_chain.to_string(),
            tx.target_chain.to_string(),
            tx.sender,
            tx.receiver,
            tx.amount.to_string(),
            tx.token_id as i64,
            serde_json::to_string(&tx.status)?,
            tx.timestamp as i64
        )
        .execute(&self.db_pool)
        .await?;
        
        Ok(())
    }
    
    // Cập nhật trạng thái của giao dịch
    async fn update_transaction_status(&self, tx_hash: &str, status: BridgeStatus) -> Result<(), Box<dyn Error>> {
        // Cập nhật trong cache
        {
            let mut transactions = self.transactions.write().await;
            if let Some(tx) = transactions.get_mut(tx_hash) {
                tx.status = status.clone();
            }
        }
        
        // Cập nhật trong database
        sqlx::query!(
            "UPDATE bridge_transactions SET status = $1 WHERE tx_hash = $2",
            serde_json::to_string(&status)?,
            tx_hash
        )
        .execute(&self.db_pool)
        .await?;
        
        Ok(())
    }

    // Monitoring transactions với exponential backoff và timeout
    pub async fn monitor_transaction<F, Fut>(
        &self,
        tx_hash: H256,
        provider_tx_hash: String,
        config: Option<MonitorConfig>,
        get_status_fn: F,
        retry_fn: impl Fn(&BridgeTransaction) -> Fut,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> 
    where
        F: Fn(&str) -> BoxFuture<'static, Result<String, Box<dyn Error>>>,
        Fut: Future<Output = Result<(), Box<dyn Error>>>,
    {
        let config = config.unwrap_or_default();
        let mut retries = 0;
        let mut delay = config.initial_delay;
        let mut total_time = 0u64;
        
        // Thêm thông tin log
        info!("Starting transaction monitoring for tx: {} with provider tx: {}", 
             tx_hash.to_string(), provider_tx_hash);
        debug!("Monitor config: max_retries={}, initial_delay={}s, backoff_factor={}, max_timeout={}s", 
              config.max_retries, config.initial_delay, config.backoff_factor, config.max_timeout);
        
        let start_time = chrono::Utc::now();
        
        loop {
            // Kiểm tra vượt quá thời gian timeout chưa
            if total_time >= config.max_timeout {
                warn!("Transaction {} monitoring timed out after {}s", tx_hash.to_string(), total_time);
                self.update_transaction_status(
                    &tx_hash.to_string(), 
                    BridgeStatus::Failed(format!("Transaction timed out after {}s", total_time))
                ).await.ok();
                
                // Thực hiện các xử lý cancel giao dịch nếu cần thiết
                metrics::increment_counter!("bridge_transactions_timeout_total", "tx_hash" => tx_hash.to_string());
                return Err(format!("Transaction timed out after {}s", total_time).into());
            }
            
            // Sleep với thời gian backoff
            debug!("Waiting {}s before checking transaction status...", delay);
            tokio::time::sleep(Duration::from_secs(delay)).await;
            total_time += delay;
            
            // Ghi nhận thời gian đã trôi qua từ khi bắt đầu
            let elapsed_time = chrono::Utc::now().signed_duration_since(start_time).num_seconds() as u64;
            metrics::gauge!("bridge_transaction_elapsed_seconds", elapsed_time as f64, 
                          "tx_hash" => tx_hash.to_string());
            
            // Kiểm tra trạng thái giao dịch
            match get_status_fn(&provider_tx_hash).await {
                Ok(status) => {
                    debug!("Transaction {} status: {}", provider_tx_hash, status);
                    
                    if status == "confirmed" || status == "success" || status == "finalized" {
                        info!("Transaction {} completed successfully after {}s", 
                             tx_hash.to_string(), elapsed_time);
                             
                        self.update_transaction_status(&tx_hash.to_string(), BridgeStatus::Completed).await
                            .unwrap_or_else(|e| {
                                error!("Failed to update transaction status: {}", e);
                            });
                            
                        metrics::increment_counter!("bridge_transactions_success_total", "tx_hash" => tx_hash.to_string());
                        break;
                    } else if status == "failed" || status == "error" || status == "rejected" {
                        warn!("Transaction {} failed with status: {}", provider_tx_hash, status);
                        
                        if retries < config.max_retries {
                            retries += 1;
                            info!("Retrying transaction {} (attempt {}/{})", 
                                 tx_hash.to_string(), retries, config.max_retries);
                                
                            // Thử lại giao dịch
                            match self.get_transaction(&tx_hash.to_string()).await {
                                Ok(tx) => {
                                    if let Err(e) = retry_fn(&tx).await {
                                        error!("Retry failed for tx {}: {}", tx_hash.to_string(), e);
                                        metrics::increment_counter!("bridge_transactions_retry_failed_total", 
                                                                 "tx_hash" => tx_hash.to_string());
                                    } else {
                                        info!("Retry successful for tx {}", tx_hash.to_string());
                                        metrics::increment_counter!("bridge_transactions_retry_success_total", 
                                                                 "tx_hash" => tx_hash.to_string());
                                    }
                                },
                                Err(e) => {
                                    error!("Failed to get transaction {} for retry: {}", tx_hash.to_string(), e);
                                }
                            }
                        } else {
                            warn!("Max retries ({}) exceeded for transaction {}", 
                                 config.max_retries, tx_hash.to_string());
                                
                            self.update_transaction_status(
                                &tx_hash.to_string(), 
                                BridgeStatus::Failed(format!("Max retries ({}) exceeded", config.max_retries))
                            ).await.ok();
                            
                            metrics::increment_counter!("bridge_transactions_max_retries_total", 
                                                     "tx_hash" => tx_hash.to_string());
                            break;
                        }
                    } else if status == "pending" || status == "processing" {
                        debug!("Transaction {} is still pending after {}s", provider_tx_hash, elapsed_time);
                    }
                },
                Err(e) => {
                    error!("Error monitoring transaction {}: {}", provider_tx_hash, e);
                    
                    if retries >= config.max_retries {
                        error!("Max retries ({}) exceeded while monitoring transaction {}", 
                              config.max_retries, tx_hash.to_string());
                              
                        self.update_transaction_status(
                            &tx_hash.to_string(), 
                            BridgeStatus::Failed(format!("Monitoring error after {} retries: {}", retries, e))
                        ).await.ok();
                        
                        metrics::increment_counter!("bridge_transactions_monitoring_error_total", 
                                                 "tx_hash" => tx_hash.to_string());
                        break;
                    }
                    
                    retries += 1;
                }
            }
            
            // Tính toán delay tiếp theo sử dụng exponential backoff
            delay = (delay as f32 * config.backoff_factor) as u64;
            
            // Đảm bảo delay không vượt quá một giới hạn hợp lý (ví dụ: 5 phút)
            delay = delay.min(300);
        }
        
        Ok(())
    }

    // Monitoring LayerZero transaction (wrapper trên monitor_transaction)
    pub async fn monitor_layerzero_transaction(&self, tx_hash: H256, lz_tx_hash: String) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        info!("Monitoring LayerZero transaction: {} (provider tx: {})", tx_hash.to_string(), lz_tx_hash);
        
        let config = MonitorConfig {
            max_retries: 5,
            initial_delay: 30,
            backoff_factor: 1.5,
            max_timeout: 3600, // 1 hour
        };
        
        let client = self.layerzero_client.clone();
        self.monitor_transaction(
            tx_hash,
            lz_tx_hash,
            Some(config),
            move |hash| {
                let client = client.clone();
                Box::pin(async move {
                    client.get_transaction_status(hash).await
                })
            },
            |tx| self.relay_to_near(tx)
        ).await
    }

    // Monitoring Wormhole transaction (wrapper trên monitor_transaction)
    pub async fn monitor_wormhole_transaction(&self, tx_hash: H256, wh_tx_hash: String) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        info!("Monitoring Wormhole transaction: {} (provider tx: {})", tx_hash.to_string(), wh_tx_hash);
        
        let config = MonitorConfig {
            max_retries: 5,
            initial_delay: 30,
            backoff_factor: 1.5,
            max_timeout: 7200, // 2 hours - Solana finality có thể lâu hơn
        };
        
        let client = self.wormhole_client.clone();
        self.monitor_transaction(
            tx_hash,
            wh_tx_hash,
            Some(config),
            move |hash| {
                let client = client.clone();
                Box::pin(async move {
                    client.get_transaction_status(hash).await
                })
            },
            |tx| self.relay_to_solana(tx)
        ).await
    }

    // Chuẩn bị payload cho LayerZero
    fn prepare_layerzero_payload(&self, tx: &BridgeTransaction) -> Result<Vec<u8>, Box<dyn Error>> {
        // Cấu trúc payload phải khớp với cách DiamondToken trên NEAR giải mã
        let payload = ethers::abi::encode(&[
            Token::Uint(tx.token_id.into()),
            Token::Uint(tx.amount),
            Token::Bytes(hex::decode(&tx.receiver)?),
            Token::Address(tx.sender.parse::<Address>()?)
        ]);
        
        Ok(payload)
    }

    // Chuẩn bị payload cho Wormhole
    async fn prepare_wormhole_payload(&self, tx: &BridgeTransaction) -> Result<Vec<u8>, Box<dyn Error>> {
        // Cấu trúc payload phải khớp với cách DiamondToken trên Solana giải mã
        let solana_program_id = self.get_solana_program_id().await?;
        
        let payload = ethers::abi::encode(&[
            Token::Uint(tx.token_id.into()),
            Token::Uint(tx.amount),
            Token::Bytes(hex::decode(&tx.receiver)?),
            Token::Address(tx.sender.parse::<Address>()?),
            Token::FixedBytes(solana_program_id.to_vec())
        ]);
        
        Ok(payload)
    }

    // Xác định chain đích dựa trên LayerZero chain ID
    fn determine_target_chain(&self, chain_id: u16) -> Result<Chain, Box<dyn Error>> {
        // Validate chain_id có hợp lý không
        if chain_id == 0 {
            error!("Invalid chain ID: 0");
            return Err("Chain ID cannot be zero".into());
        }
        
        // Kiểm tra xem chain_id có vượt quá giới hạn hợp lý không
        if chain_id > 1000 {
            warn!("Unusually high chain ID: {}", chain_id);
        }
        
        // Tìm chain tương ứng với LayerZero ID
        let chain = Chain::from_layerzero_id(chain_id);
        
        match chain {
            Some(c) => {
                debug!("Resolved chain ID {} to chain: {:?}", chain_id, c);
                Ok(c)
            },
            None => {
                // Kiểm tra có phải chain ID thuộc về testnet không
                let is_testnet = match chain_id {
                    10001 => true, // Ethereum Testnet
                    10002 => true, // BSC Testnet
                    10006 => true, // Avalanche Testnet
                    10109 => true, // Polygon Testnet
                    10115 => true, // NEAR Testnet
                    _ => false
                };
                
                if is_testnet {
                    error!("Unsupported testnet chain ID: {}. Currently only mainnet chains are supported", chain_id);
                    return Err(format!("Testnet chain ID {} is not supported. Use mainnet chains", chain_id).into());
                }
                
                error!("Unsupported chain ID: {}", chain_id);
                Err(format!("Unsupported chain ID: {}. Supported IDs are: 1 (Ethereum), 2 (BSC), 3 (Avalanche), 4 (Polygon), 115 (NEAR)", chain_id).into())
            }
        }
    }
    
    // Xác thực transaction
    async fn validate_transaction(&self, tx: &BridgeTransaction) -> Result<bool, Box<dyn Error>> {
        // Kiểm tra đầu vào
        if tx.amount.is_zero() {
            return Ok(false);
        }
        
        // Kiểm tra receiver address
        match tx.target_chain {
            Chain::NEAR => {
                // Kiểm tra định dạng NEAR account
                if !tx.receiver.contains('.') && tx.receiver.len() < 2 {
                    return Ok(false);
                }
            },
            Chain::Solana => {
                // Kiểm tra định dạng Solana address
                if hex::decode(&tx.receiver).is_err() || tx.receiver.len() != 44 {
                    return Ok(false);
                }
            },
            _ => {}
        }
        
        // Kiểm tra token ID
        if tx.token_id != 0 && tx.token_id != 1 {
            // Giả sử ID 0 là DMD token, ID 1 là token khác
            return Ok(false);
        }
        
        // Mọi thứ đều hợp lệ
        Ok(true)
    }
    
    // Lấy program ID của Solana từ smart contract
    pub async fn get_solana_program_id(&self) -> Result<Vec<u8>, Box<dyn Error>> {
        let contract = Contract::new(
            self.bridge_contract_bsc,
            include_bytes!("../abi/DmdBscBridge.json"),
            self.bsc_provider.clone(),
        );
        
        let program_id: Bytes = contract.method("solanaProgramId", ())?.call().await?;
        Ok(program_id.to_vec())
    }

    // Phương thức để lấy transaction từ cache hoặc DB
    pub async fn get_transaction(&self, tx_hash: &str) -> Result<BridgeTransaction, Box<dyn Error>> {
        // Validate tx_hash
        if tx_hash.is_empty() {
            error!("Empty transaction hash provided");
            return Err("Transaction hash cannot be empty".into());
        }
        
        // Validate định dạng tx_hash (nếu là hex)
        if tx_hash.starts_with("0x") && !tx_hash[2..].chars().all(|c| c.is_digit(16)) {
            error!("Invalid transaction hash format: {}", tx_hash);
            return Err(format!("Invalid transaction hash format: {}", tx_hash).into());
        }
        
        debug!("Looking up transaction: {}", tx_hash);
        
        // Kiểm tra trong cache
        {
            debug!("Checking transaction cache for {}", tx_hash);
            let transactions = self.transactions.read().await;
            if let Some(tx) = transactions.get(tx_hash) {
                debug!("Transaction found in cache: {} (status: {:?})", tx_hash, tx.status);
                return Ok(tx.clone());
            }
            debug!("Transaction not found in cache: {}", tx_hash);
        }
        
        // Nếu không có trong cache, lấy từ DB
        debug!("Querying database for transaction: {}", tx_hash);
        
        let record = match sqlx::query!(
            "SELECT * FROM bridge_transactions WHERE tx_hash = $1",
            tx_hash
        )
        .fetch_one(&self.db_pool)
        .await {
            Ok(record) => record,
            Err(sqlx::Error::RowNotFound) => {
                error!("Transaction not found in database: {}", tx_hash);
                return Err(format!("Transaction not found: {}", tx_hash).into());
            },
            Err(e) => {
                error!("Database error while querying transaction {}: {}", tx_hash, e);
                return Err(format!("Database error: {}", e).into());
            }
        };
        
        debug!("Transaction found in database: {} (source: {}, target: {})", 
              tx_hash, record.source_chain, record.target_chain);
        
        // Chuyển đổi thành BridgeTransaction
        let source_chain = match record.source_chain.as_str() {
            "bsc" => Chain::BSC,
            "near" => Chain::NEAR,
            "solana" => Chain::Solana,
            "ethereum" => Chain::Ethereum,
            "polygon" => Chain::Polygon,
            "avalanche" => Chain::Avalanche,
            unknown => {
                error!("Unknown source chain in database: {} for tx: {}", unknown, tx_hash);
                return Err(format!("Unknown source chain: {}", unknown).into());
            }
        };
        
        let target_chain = match record.target_chain.as_str() {
            "bsc" => Chain::BSC,
            "near" => Chain::NEAR,
            "solana" => Chain::Solana,
            "ethereum" => Chain::Ethereum, 
            "polygon" => Chain::Polygon,
            "avalanche" => Chain::Avalanche,
            unknown => {
                error!("Unknown target chain in database: {} for tx: {}", unknown, tx_hash);
                return Err(format!("Unknown target chain: {}", unknown).into());
            }
        };
        
        // Validate và parse transaction status từ DB
        let status: BridgeStatus = match serde_json::from_str(&record.status) {
            Ok(status) => status,
            Err(e) => {
                error!("Failed to parse transaction status for tx {}: {} (raw value: {})", 
                      tx_hash, e, record.status);
                return Err(format!("Invalid transaction status format: {}", e).into());
            }
        };
        
        // Validate và parse amount từ DB
        let amount = match U256::from_dec_str(&record.amount) {
            Ok(amount) => amount,
            Err(e) => {
                error!("Failed to parse amount for tx {}: {} (raw value: {})", 
                      tx_hash, e, record.amount);
                return Err(format!("Invalid amount format: {}", e).into());
            }
        };
        
        // Validate sender và receiver
        if record.sender.is_empty() {
            warn!("Empty sender address for tx: {}", tx_hash);
        }
        
        if record.receiver.is_empty() {
            warn!("Empty receiver address for tx: {}", tx_hash);
        }
        
        // Tạo transaction
        let tx = BridgeTransaction {
            tx_hash: record.tx_hash,
            source_chain,
            target_chain,
            sender: record.sender,
            receiver: record.receiver,
            amount,
            token_id: record.token_id as u64,
            status,
            timestamp: record.timestamp as u64,
        };
        
        // Validate transaction
        if let Err(e) = self.validate_transaction(&tx).await {
            warn!("Transaction {} validation warning: {}", tx_hash, e);
            // Không return error ở đây vì có thể transaction hợp lệ nhưng không đáp ứng
            // một số điều kiện validation nhất định
        }
        
        // Lưu vào cache cho lần sau
        debug!("Caching transaction: {}", tx_hash);
        {
            let mut transactions = self.transactions.write().await;
            transactions.insert(tx.tx_hash.clone(), tx.clone());
        }
        
        debug!("Successfully retrieved transaction: {} (status: {:?})", tx_hash, tx.status);
        Ok(tx)
    }
    
    // Lấy trạng thái của giao dịch - phương thức cho API
    pub async fn get_transaction_status(&self, tx_hash: &str) -> Result<BridgeStatus, Box<dyn Error>> {
        let tx = self.get_transaction(tx_hash).await?;
        Ok(tx.status.clone())
    }
    
    // Ước tính phí bridge
    pub async fn estimate_bridge_fee(
        &self,
        source_chain: Chain,
        target_chain: Chain,
        token_id: u64,
        amount: String
    ) -> Result<FeeEstimate, Box<dyn Error>> {
        // Validate input parameters
        if amount.is_empty() {
            error!("Empty amount provided for fee estimation");
            return Err("Amount cannot be empty".into());
        }
        
        if token_id > 100 {
            warn!("Unusual token ID: {}. Typical token IDs are 0 or 1", token_id);
        }
        
        debug!("Estimating bridge fee from {} to {} for token_id={} and amount={}",
              source_chain.as_str(), target_chain.as_str(), token_id, amount);
        
        // Validate chain compatibility
        if source_chain == target_chain {
            error!("Source and target chains are the same: {}", source_chain.as_str());
            return Err("Source and target chains cannot be the same".into());
        }
        
        // Validate chain support for bridges
        let using_layerzero = match (source_chain, target_chain) {
            (Chain::BSC, Chain::NEAR) | 
            (Chain::Ethereum, Chain::NEAR) | 
            (Chain::Polygon, Chain::NEAR) |
            (Chain::Avalanche, Chain::NEAR) => {
                if !target_chain.is_layerzero_supported() {
                    error!("Target chain {} is not supported by LayerZero", target_chain.as_str());
                    return Err(format!("Chain {} is not supported by LayerZero", target_chain.as_str()).into());
                }
                true
            },
            (Chain::BSC, Chain::Solana) |
            (Chain::Ethereum, Chain::Solana) |
            (Chain::Polygon, Chain::Solana) |
            (Chain::Avalanche, Chain::Solana) => {
                if !target_chain.is_wormhole_supported() {
                    error!("Target chain {} is not supported by Wormhole", target_chain.as_str());
                    return Err(format!("Chain {} is not supported by Wormhole", target_chain.as_str()).into());
                }
                false
            },
            _ => {
                error!("Unsupported chain combination: {} -> {}", 
                      source_chain.as_str(), target_chain.as_str());
                return Err(format!(
                    "Unsupported chain combination: {} -> {}. Supported: BSC/ETH/Polygon/Avalanche -> NEAR/Solana", 
                    source_chain.as_str(), target_chain.as_str()
                ).into());
            }
        };
        
        // Chuyển đổi amount thành U256
        let amount_u256 = match U256::from_dec_str(&amount) {
            Ok(amount) => amount,
            Err(e) => {
                error!("Failed to parse amount '{}': {}", amount, e);
                return Err(format!("Invalid amount format: {}", e).into());
            }
        };
        
        // Chuẩn bị payload giả để ước tính kích thước
        let mock_payload = match target_chain {
            Chain::NEAR => {
                debug!("Preparing mock LayerZero payload for NEAR");
                // Mô phỏng payload LayerZero
                ethers::abi::encode(&[
                    Token::Uint(token_id.into()),
                    Token::Uint(amount_u256),
                    Token::Bytes(vec![0; 32]), // Giả lập receiver
                    Token::Address(Address::zero())
                ])
            },
            Chain::Solana => {
                debug!("Preparing mock Wormhole payload for Solana");
                // Mô phỏng payload Wormhole
                ethers::abi::encode(&[
                    Token::Uint(token_id.into()),
                    Token::Uint(amount_u256),
                    Token::Bytes(vec![0; 32]), // Giả lập receiver
                    Token::Address(Address::zero()),
                    Token::FixedBytes(vec![0; 32]) // Giả lập program ID
                ])
            },
            _ => {
                error!("Unsupported target chain for fee estimation: {}", target_chain.as_str());
                return Err(format!("Unsupported target chain for fee estimation: {}", target_chain.as_str()).into());
            }
        };
        
        debug!("Mock payload size: {} bytes", mock_payload.len());
        
        // Ước tính phí dựa vào loại bridge
        let result = if using_layerzero {
            debug!("Using LayerZero for fee estimation");
            self.layerzero_client.estimate_fee(
                source_chain.to_layerzero_id(),
                target_chain.to_layerzero_id(),
                mock_payload.len()
            ).await
        } else {
            debug!("Using Wormhole for fee estimation");
            self.wormhole_client.estimate_fee(
                source_chain.as_str(),
                target_chain.as_str(),
                mock_payload.len()
            ).await
        };
        
        match result {
            Ok(fee) => {
                info!("Fee estimated successfully: {} {} (${} USD) for {} -> {}", 
                     fee.fee_amount, fee.fee_token, fee.fee_usd,
                     source_chain.as_str(), target_chain.as_str());
                Ok(fee)
            },
            Err(e) => {
                error!("Fee estimation failed for {} -> {}: {}", 
                      source_chain.as_str(), target_chain.as_str(), e);
                Err(format!("Fee estimation failed: {}", e).into())
            }
        }
    }
}

// Thêm Clone cho BridgeOrchestrator
impl Clone for BridgeOrchestrator {
    fn clone(&self) -> Self {
        // Việc clone cho BridgeOrchestrator khá phức tạp vì có nhiều thành phần
        // Trong thực tế, có thể cân nhắc sử dụng Arc thay vì clone
        unimplemented!("Clone not fully implemented for BridgeOrchestrator")
    }
}


