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
use log;
use std::collections::HashMap;
use async_trait::async_trait;
use sqlx::PgPool;
use serde::{Serialize, Deserialize};

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
        tokio::spawn(self.clone().monitor_layerzero_transaction(tx.tx_hash.clone(), lz_tx));
        
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
        tokio::spawn(self.clone().monitor_wormhole_transaction(tx.tx_hash.clone(), wh_tx));
        
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

    // Monitoring và retry LayerZero
    pub async fn monitor_layerzero_transaction(&self, tx_hash: H256, lz_tx_hash: String) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let max_retries = 3;
        let mut retries = 0;
        
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            
            match self.layerzero_client.get_transaction_status(&lz_tx_hash).await {
                Ok(status) => {
                    if status == "confirmed" {
                        self.update_transaction_status(&tx_hash.to_string(), BridgeStatus::Completed).await.ok();
                        break;
                    } else if status == "failed" {
                        if retries < max_retries {
                            retries += 1;
                            // Thử lại giao dịch
                            if let Ok(tx) = self.get_transaction(&tx_hash.to_string()).await {
                                if let Err(e) = self.relay_to_near(&tx).await {
                                    log::error!("Retry failed: {}", e);
                                }
                            }
                        } else {
                            self.update_transaction_status(
                                &tx_hash.to_string(), 
                                BridgeStatus::Failed("Max retries exceeded".into())
                            ).await.ok();
                            break;
                        }
                    }
                },
                Err(e) => {
                    log::error!("Error monitoring LayerZero transaction: {}", e);
                    if retries >= max_retries {
                        self.update_transaction_status(
                            &tx_hash.to_string(), 
                            BridgeStatus::Failed(format!("Monitoring error: {}", e))
                        ).await.ok();
                        break;
                    }
                    retries += 1;
                }
            }
        }
    }

    // Monitoring và retry Wormhole
    pub async fn monitor_wormhole_transaction(&self, tx_hash: H256, wh_tx_hash: String) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let max_retries = 3;
        let mut retries = 0;
        
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            
            match self.wormhole_client.get_transaction_status(&wh_tx_hash).await {
                Ok(status) => {
                    if status == "confirmed" {
                        self.update_transaction_status(&tx_hash.to_string(), BridgeStatus::Completed).await.ok();
                        break;
                    } else if status == "failed" {
                        if retries < max_retries {
                            retries += 1;
                            // Thử lại giao dịch
                            if let Ok(tx) = self.get_transaction(&tx_hash.to_string()).await {
                                if let Err(e) = self.relay_to_solana(&tx).await {
                                    log::error!("Retry failed: {}", e);
                                }
                            }
                        } else {
                            self.update_transaction_status(
                                &tx_hash.to_string(), 
                                BridgeStatus::Failed("Max retries exceeded".into())
                            ).await.ok();
                            break;
                        }
                    }
                },
                Err(e) => {
                    log::error!("Error monitoring Wormhole transaction: {}", e);
                    if retries >= max_retries {
                        self.update_transaction_status(
                            &tx_hash.to_string(), 
                            BridgeStatus::Failed(format!("Monitoring error: {}", e))
                        ).await.ok();
                        break;
                    }
                    retries += 1;
                }
            }
        }
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
        Chain::from_layerzero_id(chain_id)
            .ok_or_else(|| format!("Unsupported chain ID: {}", chain_id).into())
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
        // Kiểm tra trong cache
        {
            let transactions = self.transactions.read().await;
            if let Some(tx) = transactions.get(tx_hash) {
                return Ok(tx.clone());
            }
        }
        
        // Nếu không có trong cache, lấy từ DB
        let record = sqlx::query!(
            "SELECT * FROM bridge_transactions WHERE tx_hash = $1",
            tx_hash
        )
        .fetch_one(&self.db_pool)
        .await?;
        
        // Chuyển đổi thành BridgeTransaction
        let source_chain = match record.source_chain.as_str() {
            "bsc" => Chain::BSC,
            "near" => Chain::NEAR,
            "solana" => Chain::Solana,
            "ethereum" => Chain::Ethereum,
            "polygon" => Chain::Polygon,
            "avalanche" => Chain::Avalanche,
            _ => return Err("Unknown source chain".into()),
        };
        
        let target_chain = match record.target_chain.as_str() {
            "bsc" => Chain::BSC,
            "near" => Chain::NEAR,
            "solana" => Chain::Solana,
            "ethereum" => Chain::Ethereum, 
            "polygon" => Chain::Polygon,
            "avalanche" => Chain::Avalanche,
            _ => return Err("Unknown target chain".into()),
        };
        
        let status: BridgeStatus = serde_json::from_str(&record.status)?;
        
        let tx = BridgeTransaction {
            tx_hash: record.tx_hash,
            source_chain,
            target_chain,
            sender: record.sender,
            receiver: record.receiver,
            amount: U256::from_dec_str(&record.amount)?,
            token_id: record.token_id as u64,
            status,
            timestamp: record.timestamp as u64,
        };
        
        // Lưu vào cache cho lần sau
        {
            let mut transactions = self.transactions.write().await;
            transactions.insert(tx.tx_hash.clone(), tx.clone());
        }
        
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
        // Chuyển đổi amount thành U256
        let amount_u256 = U256::from_dec_str(&amount)?;
        
        // Chuẩn bị payload giả để ước tính kích thước
        let mock_payload = match target_chain {
            Chain::NEAR => {
                // Mô phỏng payload LayerZero
                ethers::abi::encode(&[
                    Token::Uint(token_id.into()),
                    Token::Uint(amount_u256),
                    Token::Bytes(vec![0; 32]), // Giả lập receiver
                    Token::Address(Address::zero())
                ])
            },
            Chain::Solana => {
                // Mô phỏng payload Wormhole
                ethers::abi::encode(&[
                    Token::Uint(token_id.into()),
                    Token::Uint(amount_u256),
                    Token::Bytes(vec![0; 32]), // Giả lập receiver
                    Token::Address(Address::zero()),
                    Token::FixedBytes(vec![0; 32]) // Giả lập program ID
                ])
            },
            _ => return Err("Unsupported target chain for fee estimation".into()),
        };
        
        // Ước tính phí dựa vào loại bridge
        match (source_chain, target_chain) {
            (Chain::BSC, Chain::NEAR) => {
                self.layerzero_client.estimate_fee(
                    source_chain.to_layerzero_id(),
                    target_chain.to_layerzero_id(),
                    mock_payload.len()
                ).await
            },
            (Chain::BSC, Chain::Solana) => {
                self.wormhole_client.estimate_fee(
                    source_chain.as_str(),
                    target_chain.as_str(),
                    mock_payload.len()
                ).await
            },
            _ => Err("Unsupported chain combination for fee estimation".into()),
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


