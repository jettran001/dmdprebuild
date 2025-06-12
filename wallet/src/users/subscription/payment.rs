//! Module quản lý thanh toán cho các gói đăng ký.
//!
//! Module này cung cấp các chức năng liên quan đến việc xử lý thanh toán cho các gói đăng ký,
//! bao gồm:
//! - Xử lý giao dịch thanh toán từ người dùng
//! - Kiểm tra trạng thái giao dịch trên blockchain
//! - Xác minh số tiền thanh toán và loại token phù hợp với gói đăng ký
//! - Xử lý lỗi liên quan đến blockchain
//! - Quản lý việc gia hạn đăng ký
//!
//! Module này tương tác với các module khác như user_subscription và staking, cũng như
//! các dịch vụ blockchain bên ngoài để đảm bảo quá trình thanh toán diễn ra trơn tru và an toàn.

// External imports
use uuid::Uuid;
use ethers::types::{Address, U256, H256, U64, Transaction, TransactionReceipt};
use chrono::{DateTime, Utc, Duration};
use serde::{Serialize, Deserialize};
use tokio::sync::RwLock;
use async_trait::async_trait;
use tracing::{info, error, warn, debug};
use std::time::Duration as StdDuration;
use backoff::{ExponentialBackoff, retry, Error as BackoffError};
use rand;

// Internal imports
use crate::error::{WalletError, Result as AppResult};
use crate::walletmanager::api::WalletManagerApi;
use crate::defi::types::{ChainType, TokenAddress};
use super::types::{SubscriptionType, SubscriptionStatus, PaymentToken};

use crate::blockchain::types::TokenInfo;
use super::constants::{
    BLOCKCHAIN_TX_CONFIRMATION_BLOCKS, 
    BLOCKCHAIN_TX_RETRY_ATTEMPTS, 
    BLOCKCHAIN_TX_RETRY_DELAY_MS
};
use super::types::{TransactionCheckResult};
use super::utils::{calculate_payment_amount, validate_transaction_hash, log_subscription_activity};

/// Thông tin về giao dịch thanh toán
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionInfo {
    /// ID giao dịch
    pub id: Uuid,
    /// Hash giao dịch blockchain
    pub tx_hash: String,
    /// Địa chỉ ví người gửi
    pub from_address: Address,
    /// Địa chỉ ví người nhận
    pub to_address: Address,
    /// Token dùng để thanh toán
    pub token: PaymentToken,
    /// Số lượng token
    pub amount: U256,
    /// Trạng thái giao dịch
    pub status: TransactionStatus,
    /// Thời điểm tạo giao dịch
    pub created_at: DateTime<Utc>,
    /// Thời điểm cập nhật giao dịch
    pub updated_at: DateTime<Utc>,
}

/// Trạng thái giao dịch
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionStatus {
    /// Đang chờ xử lý
    Pending,
    /// Đã xác nhận
    Confirmed,
    /// Thất bại
    Failed,
    /// Đã hết hạn
    Expired,
}

impl Default for TransactionStatus {
    fn default() -> Self {
        Self::Pending
    }
}

/// Processor xử lý thanh toán
#[derive(Debug)]
pub struct PaymentProcessor {
    /// ID chuỗi blockchain đang sử dụng (BSC, Ethereum, v.v.)
    chain_id: u64,
    /// Địa chỉ ví nhận thanh toán
    payment_receiver: Address,
    /// Địa chỉ hợp đồng token DMD
    dmd_token_address: Address,
    /// Địa chỉ hợp đồng token USDC
    usdc_token_address: Address,
}

impl PaymentProcessor {
    /// Tạo processor mới
    pub fn new(
        chain_id: u64,
        payment_receiver: Address,
        dmd_token_address: Address,
        usdc_token_address: Address,
    ) -> Self {
        Self {
            chain_id,
            payment_receiver,
            dmd_token_address,
            usdc_token_address,
        }
    }
    
    /// Khởi tạo giao dịch thanh toán mới
    pub fn create_payment_transaction(
        &self,
        from_address: Address,
        payment_token: PaymentToken,
        amount: U256,
    ) -> TransactionInfo {
        let now = Utc::now();
        
        TransactionInfo {
            id: Uuid::new_v4(),
            tx_hash: String::new(), // Chưa có hash giao dịch
            from_address,
            to_address: self.payment_receiver,
            token: payment_token,
            amount,
            status: TransactionStatus::Pending,
            created_at: now,
            updated_at: now,
        }
    }
    
    /// Lấy provider blockchain với retry và xử lý lỗi
    async fn get_blockchain_provider(&self) -> Result<BlockchainProvider, WalletError> {
        // Thêm retry mechanism khi kết nối blockchain
        const MAX_RETRIES: u8 = 3;
        let mut last_error: Option<String> = None;
        
        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                // Exponential backoff với jitter
                let backoff_ms = 100 * u64::pow(2, attempt as u32);
                let jitter = rand::random::<u64>() % 100;
                let delay = backoff_ms + jitter;
                
                debug!("Thử kết nối lại blockchain lần {}/{} sau {}ms", attempt, MAX_RETRIES, delay);
                tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
            }
            
            // Thực hiện kết nối đến blockchain
            match BlockchainProvider::new(self.chain_id) {
                Ok(provider) => {
                    // Kiểm tra kết nối
                    match provider.is_connected().await {
                        true => {
                            debug!("Kết nối thành công đến blockchain với chain_id: {}", self.chain_id);
                            return Ok(provider);
                        },
                        false => {
                            warn!("Không thể kết nối đến blockchain với chain_id: {}", self.chain_id);
                            last_error = Some(format!("Không thể kết nối đến blockchain với chain_id: {}", self.chain_id));
                            continue;
                        }
                    }
                },
                Err(e) => {
                    error!("Lỗi khởi tạo provider blockchain: {}", e);
                    last_error = Some(format!("Lỗi khởi tạo provider: {}", e));
                    
                    if attempt >= MAX_RETRIES {
                        return Err(WalletError::BlockchainConnectionError(format!(
                            "Không thể kết nối đến blockchain sau {} lần thử: {}", 
                            MAX_RETRIES + 1, last_error.unwrap_or_default()
                        )));
                    }
                }
            }
        }
        
        // Nếu đến đây thì đã hết số lần thử nhưng không thành công
        Err(WalletError::BlockchainConnectionError(format!(
            "Không thể kết nối đến blockchain sau {} lần thử: {}", 
            MAX_RETRIES + 1, last_error.unwrap_or_default()
        )))
    }
    
    /// Kiểm tra trạng thái giao dịch trên blockchain với cơ chế retry và logging chi tiết
    pub async fn check_transaction_status(
        &self,
        tx_hash: &str,
    ) -> AppResult<TransactionCheckResult> {
        // Validate đầu vào
        if !validate_transaction_hash(tx_hash) {
            error!("Hash giao dịch không hợp lệ: {}", tx_hash);
            return Err(WalletError::InvalidTransactionHash.into());
        }
        
        info!("Bắt đầu kiểm tra giao dịch: {}", tx_hash);
        
        // Thực hiện kiểm tra với số lần thử quy định
        let mut last_error: Option<WalletError> = None;
        
        for attempt in 0..BLOCKCHAIN_TX_RETRY_ATTEMPTS {
            // Log attempt
            if attempt > 0 {
                info!("Thử kiểm tra giao dịch {} lần thứ {}/{}", 
                     tx_hash, attempt + 1, BLOCKCHAIN_TX_RETRY_ATTEMPTS);
                
                // Thêm exponential backoff với jitter để tránh thundering herd
                let backoff_ms = BLOCKCHAIN_TX_RETRY_DELAY_MS * u64::pow(2, attempt as u32);
                let jitter = rand::random::<u64>() % 100;
                let delay = backoff_ms + jitter;
                
                debug!("Chờ {}ms trước khi thử lại", delay);
                tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
            }
            
            match self.check_transaction_once(tx_hash).await {
                Ok(result) => {
                    // Nếu giao dịch đang pending và chưa phải lần thử cuối, tiếp tục đợi
                    if matches!(result, TransactionCheckResult::Pending) && 
                       attempt < BLOCKCHAIN_TX_RETRY_ATTEMPTS - 1 {
                        debug!("Giao dịch {} vẫn đang pending, tiếp tục đợi", tx_hash);
                        continue;
                    }
                    
                    // Log kết quả cuối cùng
                    match &result {
                        TransactionCheckResult::Confirmed { plan, .. } => {
                            info!("Giao dịch {} đã được xác nhận thành công, gói: {:?}", tx_hash, plan);
                        },
                        TransactionCheckResult::Failed { reason } => {
                            warn!("Giao dịch {} thất bại, lý do: {}", tx_hash, reason);
                        },
                        TransactionCheckResult::Pending => {
                            warn!("Giao dịch {} vẫn đang pending sau {} lần thử", tx_hash, attempt + 1);
                        },
                        TransactionCheckResult::Timeout => {
                            warn!("Timeout khi kiểm tra giao dịch {} sau {} lần thử", tx_hash, attempt + 1);
                        }
                    }
                    
                    return Ok(result);
                },
                Err(e) => {
                    // Ghi log chi tiết về lỗi
                    last_error = Some(e.clone());
                    error!("Lỗi khi kiểm tra giao dịch {} lần thứ {}/{}: {}", 
                          tx_hash, attempt + 1, BLOCKCHAIN_TX_RETRY_ATTEMPTS, e);
                    
                    // Phân loại lỗi để quyết định có nên thử lại
                    match &e {
                        // Các lỗi mạng và timeout có thể thử lại
                        WalletError::NetworkError(_) | 
                        WalletError::TimeoutError(_) | 
                        WalletError::BlockchainConnectionError(_) => {
                            if attempt < BLOCKCHAIN_TX_RETRY_ATTEMPTS - 1 {
                                warn!("Sẽ thử lại kiểm tra giao dịch {}", tx_hash);
                                continue;
                            }
                        },
                        // Các lỗi khác không nên thử lại
                        _ => {
                            error!("Lỗi không thể thử lại khi kiểm tra giao dịch {}: {}", tx_hash, e);
                            return Err(e.into());
                        }
                    }
                }
            }
        }
        
        // Nếu đã thử đủ số lần mà vẫn gặp lỗi, trả về lỗi cuối cùng hoặc Timeout
        if let Some(e) = last_error {
            error!("Đã thử đủ {} lần nhưng vẫn gặp lỗi khi kiểm tra giao dịch {}: {}", 
                  BLOCKCHAIN_TX_RETRY_ATTEMPTS, tx_hash, e);
            Err(e.into())
        } else {
            warn!("Đã thử đủ {} lần, không có kết quả rõ ràng cho giao dịch {}", 
                 BLOCKCHAIN_TX_RETRY_ATTEMPTS, tx_hash);
            Ok(TransactionCheckResult::Timeout)
        }
    }
    
    /// Kiểm tra giao dịch một lần
    async fn check_transaction_once(&self, tx_hash: &str) -> Result<TransactionCheckResult, WalletError> {
        // Kiểm tra tính hợp lệ của hash giao dịch
        if !validate_transaction_hash(tx_hash) {
            error!("Hash giao dịch không hợp lệ: {}", tx_hash);
            return Err(WalletError::InvalidTransactionHash);
        }

        debug!("Bắt đầu kiểm tra giao dịch: {}", tx_hash);

        // Backoff strategy cho các cuộc gọi blockchain
        let mut backoff = ExponentialBackoff {
            initial_interval: StdDuration::from_millis(100),
            max_interval: StdDuration::from_secs(2),
            multiplier: 1.5,
            max_elapsed_time: Some(StdDuration::from_secs(10)),
            ..ExponentialBackoff::default()
        };

        // Thử kết nối đến blockchain với backoff strategy
        let provider = match retry(backoff.clone(), || async {
            match self.get_blockchain_provider().await {
                Ok(provider) => Ok(provider),
                Err(e) => {
                    warn!("Không thể kết nối đến blockchain, thử lại: {}", e);
                    Err(WalletError::BlockchainConnectionError(e.to_string()))
                }
            }
        }).await {
            Ok(provider) => provider,
            Err(e) => {
                error!("Không thể kết nối đến blockchain sau nhiều lần thử: {}", e);
                return Err(WalletError::BlockchainConnectionError(e.to_string()));
            }
        };

        // Truy vấn thông tin giao dịch từ blockchain
        let transaction = match retry(backoff.clone(), || async {
            match provider.get_transaction(tx_hash).await {
                Ok(Some(tx)) => Ok(tx),
                Ok(None) => {
                    debug!("Giao dịch chưa được tìm thấy trên blockchain, có thể đang pending: {}", tx_hash);
                    Err(WalletError::TransactionNotFound(tx_hash.to_string()))
                },
                Err(e) => {
                    warn!("Lỗi khi truy vấn giao dịch {}, thử lại: {}", tx_hash, e);
                    Err(WalletError::BlockchainQueryError(e.to_string()))
                }
            }
        }).await {
            Ok(tx) => tx,
            Err(WalletError::TransactionNotFound(_)) => {
                // Nếu không tìm thấy giao dịch, có thể đang pending
                debug!("Giao dịch {} không tìm thấy sau nhiều lần thử, trả về Pending", tx_hash);
                return Ok(TransactionCheckResult::Pending);
            },
            Err(e) => {
                error!("Không thể truy vấn thông tin giao dịch {}: {}", tx_hash, e);
                return Err(e);
            }
        };

        // Kiểm tra xem giao dịch đã được xác nhận chưa
        if transaction.block_number.is_none() {
            debug!("Giao dịch {} đã được gửi nhưng chưa được xác nhận (block_number is None)", tx_hash);
            return Ok(TransactionCheckResult::Pending);
        }

        // Truy vấn biên lai giao dịch để kiểm tra status
        let receipt = match retry(backoff, || async {
            match provider.get_transaction_receipt(tx_hash).await {
                Ok(Some(receipt)) => Ok(receipt),
                Ok(None) => {
                    debug!("Biên lai giao dịch {} chưa sẵn sàng, có thể đang pending", tx_hash);
                    Err(WalletError::TransactionReceiptNotFound(tx_hash.to_string()))
                },
                Err(e) => {
                    warn!("Lỗi khi truy vấn biên lai giao dịch {}, thử lại: {}", tx_hash, e);
                    Err(WalletError::BlockchainQueryError(e.to_string()))
                }
            }
        }).await {
            Ok(receipt) => receipt,
            Err(WalletError::TransactionReceiptNotFound(_)) => {
                // Nếu không tìm thấy biên lai, giao dịch vẫn đang pending
                debug!("Biên lai giao dịch {} không tìm thấy sau nhiều lần thử, trả về Pending", tx_hash);
                return Ok(TransactionCheckResult::Pending);
            },
            Err(e) => {
                error!("Không thể truy vấn biên lai giao dịch {}: {}", tx_hash, e);
                return Err(e);
            }
        };

        // Kiểm tra status của giao dịch
        match receipt.status {
            Some(status) if status == U64::from(1) => {
                info!("Giao dịch {} đã được xác nhận thành công", tx_hash);
                
                // Lấy thông tin token và số lượng
                let token_info = match self.get_token_info_from_tx(tx_hash).await {
                    Ok(info) => info,
                    Err(e) => {
                        error!("Không thể lấy thông tin token từ giao dịch {}: {:?}", tx_hash, e);
                        return Err(WalletError::BlockchainQueryError(format!("Không thể lấy thông tin token: {}", e)));
                    }
                };
                
                let amount = match self.get_transaction_amount(tx_hash).await {
                    Ok(amount) => amount,
                    Err(e) => {
                        error!("Không thể lấy số lượng token từ giao dịch {}: {:?}", tx_hash, e);
                        return Err(WalletError::BlockchainQueryError(format!("Không thể lấy số lượng token: {}", e)));
                    }
                };
                
                let plan = self.determine_subscription_plan(&amount, &token_info).map_err(|e| {
                    error!("Không thể xác định gói đăng ký từ giao dịch {}: {:?}", tx_hash, e);
                    WalletError::InvalidSubscriptionPlan
                })?;
                
                Ok(TransactionCheckResult::Confirmed {
                    token: token_info,
                    amount,
                    plan,
                    tx_id: tx_hash.to_string(),
                })
            },
            Some(status) if status == U64::from(0) => {
                warn!("Giao dịch {} đã thất bại", tx_hash);
                
                // Lấy lý do thất bại
                let reason = match self.get_transaction_failure_reason(tx_hash).await {
                    Ok(reason) => reason,
                    Err(e) => {
                        error!("Không thể lấy lý do thất bại từ giao dịch {}: {:?}", tx_hash, e);
                        "Không xác định được lý do thất bại".to_string()
                    }
                };
                
                Ok(TransactionCheckResult::Failed { reason })
            },
            _ => {
                warn!("Giao dịch {} có trạng thái không xác định", tx_hash);
                Ok(TransactionCheckResult::Pending)
            }
        }
    }
    
    /// Lấy thông tin token từ giao dịch
    async fn get_token_info_from_tx(&self, tx_hash: &str) -> Result<TokenInfo, WalletError> {
        // TODO: Triển khai chi tiết kết nối blockchain và lấy thông tin token
        // Đây là code mẫu để giả lập
        
        // Mô phỏng một số lỗi ngẫu nhiên để kiểm tra xử lý lỗi
        if tx_hash.ends_with("ff") {
            return Err(WalletError::BlockchainQueryError("Lỗi truy vấn thông tin token".to_string()));
        }
        
        Ok(TokenInfo {
            address: self.usdc_token_address,
            name: "USDC".to_string(),
            symbol: "USDC".to_string(),
            decimals: 6,
        })
    }
    
    /// Lấy số lượng token từ giao dịch
    async fn get_transaction_amount(&self, tx_hash: &str) -> Result<U256, WalletError> {
        // Cải thiện: Thêm cơ chế retry và exponential backoff
        const MAX_RETRIES: u8 = 3;
        let mut last_error: Option<WalletError> = None;
        
        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                // Exponential backoff với jitter
                let backoff_ms = 100 * u64::pow(2, attempt as u32);
                let jitter = rand::random::<u64>() % 100;
                let delay = backoff_ms + jitter;
                
                info!("Thử lại lần {}/{} để lấy số lượng token cho giao dịch {} sau {}ms", 
                     attempt, MAX_RETRIES, tx_hash, delay);
                tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
            }
            
            // Thực hiện gọi API blockchain
            // Mô phỏng một số lỗi ngẫu nhiên để kiểm tra xử lý lỗi
            if tx_hash.ends_with("ee") {
                // Ghi log lỗi chi tiết
                error!("Lỗi truy vấn số lượng token cho giao dịch {}", tx_hash);
                last_error = Some(WalletError::BlockchainQueryError("Lỗi truy vấn số lượng token".to_string()));
                continue;
            }
            
            // Mô phỏng trả về kết quả thành công
            return Ok(U256::from(100));
        }
        
        // Nếu đã hết số lần thử, trả về lỗi cuối cùng hoặc lỗi mặc định
        Err(last_error.unwrap_or_else(|| WalletError::BlockchainQueryError(
            format!("Không thể lấy số lượng token sau {} lần thử", MAX_RETRIES + 1)
        )))
    }
    
    /// Xác định gói đăng ký dựa trên số lượng token và loại token
    fn determine_subscription_plan(&self, amount: &U256, token_info: &TokenInfo) -> Result<SubscriptionType, WalletError> {
        // TODO: Triển khai logic xác định gói đăng ký dựa trên số lượng token
        // Đây là code mẫu để giả lập
        
        if amount >= &U256::from(1000) {
            Ok(SubscriptionType::VIP)
        } else if amount >= &U256::from(100) {
            Ok(SubscriptionType::Premium)
        } else if amount >= &U256::from(10) {
            Ok(SubscriptionType::Free)
        } else {
            Err(WalletError::InsufficientPaymentAmount)
        }
    }
    
    /// Lấy lý do thất bại của giao dịch
    async fn get_transaction_failure_reason(&self, tx_hash: &str) -> Result<String, WalletError> {
        // TODO: Triển khai chi tiết kết nối blockchain và lấy lý do thất bại
        // Đây là code mẫu để giả lập
        
        // Mô phỏng một số lỗi ngẫu nhiên để kiểm tra xử lý lỗi
        if tx_hash.ends_with("dd") {
            return Err(WalletError::BlockchainQueryError("Lỗi truy vấn lý do thất bại".to_string()));
        }
        
        if tx_hash.ends_with("1") {
            Ok("Số dư không đủ".to_string())
        } else if tx_hash.ends_with("2") {
            Ok("Gas price quá thấp".to_string())
        } else if tx_hash.ends_with("3") {
            Ok("Giao dịch bị hủy bởi người dùng".to_string())
        } else {
            Ok("Giao dịch bị từ chối bởi mạng".to_string())
        }
    }
    
    /// Xác nhận giao dịch đã được xử lý và cập nhật thông tin
    pub fn confirm_transaction(
        &self,
        transaction: &mut TransactionInfo,
        receipt: TransactionReceipt,
    ) -> AppResult<()> {
        let now = Utc::now();
        
        if receipt.status.unwrap_or_default().as_u64() == 1 {
            transaction.status = TransactionStatus::Confirmed;
        } else {
            transaction.status = TransactionStatus::Failed;
        }
        
        transaction.updated_at = now;
        
        Ok(())
    }
    
    /// Lấy địa chỉ hợp đồng token theo loại
    pub fn get_token_address(&self, token: PaymentToken) -> Address {
        match token {
            PaymentToken::DMD => self.dmd_token_address,
            PaymentToken::USDC => self.usdc_token_address,
        }
    }
}

/// Thông tin giao dịch thanh toán
#[derive(Debug, Clone)]
pub struct PaymentTransaction {
    /// ID người dùng
    pub user_id: String,
    /// Loại đăng ký
    pub subscription_type: SubscriptionType,
    /// Hash giao dịch
    pub tx_hash: H256,
    /// Địa chỉ ví thanh toán
    pub from_address: Address,
    /// Loại token thanh toán
    pub payment_token: PaymentToken,
    /// Số lượng token
    pub amount: U256,
    /// Thời gian bắt đầu giao dịch
    pub timestamp: DateTime<Utc>,
    /// Trạng thái giao dịch
    pub status: TransactionStatus,
    /// Số lần thử lại kiểm tra
    pub retry_count: u8,
}

/// Manager xử lý thanh toán đăng ký
pub struct PaymentManager {
    /// Wallet manager API
    wallet_manager: Arc<dyn WalletManagerApi>,
    /// Danh sách giao dịch đang chờ xử lý
    pending_transactions: RwLock<Vec<PaymentTransaction>>,
}

impl PaymentManager {
    /// Tạo một PaymentManager mới
    ///
    /// # Arguments
    /// * `wallet_manager` - API quản lý ví
    pub fn new(wallet_manager: Arc<dyn WalletManagerApi>) -> Self {
        Self {
            wallet_manager,
            pending_transactions: RwLock::new(Vec::new()),
        }
    }

    /// Xử lý giao dịch thanh toán mới
    ///
    /// # Arguments
    /// * `user_id` - ID người dùng
    /// * `subscription_type` - Loại đăng ký
    /// * `payment_token` - Loại token thanh toán
    /// * `tx_hash` - Hash giao dịch
    /// * `from_address` - Địa chỉ ví thanh toán
    pub async fn process_payment(
        &self,
        user_id: &str,
        subscription_type: SubscriptionType,
        payment_token: PaymentToken,
        tx_hash: &str,
        from_address: Address,
    ) -> Result<(), WalletError> {
        // Xác thực hash giao dịch
        if !validate_transaction_hash(tx_hash) {
            return Err(WalletError::InvalidTransactionHash);
        }
        
        // Chuyển đổi hash thành H256
        let hash_bytes = match hex::decode(&tx_hash[2..]) {
            Ok(bytes) => bytes,
            Err(_) => return Err(WalletError::InvalidTransactionHash),
        };
        
        let mut hash = [0u8; 32];
        if hash_bytes.len() == 32 {
            hash.copy_from_slice(&hash_bytes);
        } else {
            return Err(WalletError::InvalidTransactionHash);
        }
        
        let tx_hash = H256::from(hash);
        
        // Kiểm tra xem giao dịch đã tồn tại chưa
        {
            let transactions = self.pending_transactions.read().await;
            if transactions.iter().any(|tx| tx.tx_hash == tx_hash) {
                return Err(WalletError::TransactionAlreadyProcessed);
            }
        }
        
        // Tính toán số lượng token cần thiết
        let required_amount = calculate_payment_amount(subscription_type, payment_token);
        
        // Tạo thông tin giao dịch mới
        let transaction = PaymentTransaction {
            user_id: user_id.to_string(),
            subscription_type,
            tx_hash,
            from_address,
            payment_token,
            amount: required_amount,
            timestamp: Utc::now(),
            status: TransactionStatus::Pending,
            retry_count: 0,
        };
        
        // Lưu giao dịch vào danh sách đang chờ xử lý
        {
            let mut transactions = self.pending_transactions.write().await;
            transactions.push(transaction);
        }
        
        // Ghi log hoạt động
        log_subscription_activity(
            user_id,
            format!("Đã nhận yêu cầu thanh toán: {:?}", tx_hash),
        );
        
        Ok(())
    }

    /// Kiểm tra tất cả các giao dịch đang chờ xử lý
    pub async fn check_pending_transactions(&self) -> Result<(), WalletError> {
        // Lấy danh sách giao dịch đang chờ xử lý
        let transactions = self.pending_transactions.read().await.clone();
        
        for tx in transactions {
            // Bỏ qua các giao dịch đã xác nhận hoặc thất bại
            if tx.status != TransactionStatus::Pending {
                continue;
            }
            
            match self.check_transaction_status(&tx).await {
                Ok(new_status) => {
                    if new_status != tx.status {
                        let new_retry_count = tx.retry_count + 1;
                        self.update_transaction_status(&tx.tx_hash, new_status, new_retry_count).await;
                        
                        // Ghi log
                        info!(
                            "Cập nhật trạng thái giao dịch {} cho user {}: {:?} -> {:?}",
                            tx.tx_hash, tx.user_id, tx.status, new_status
                        );
                        
                        // Ghi log hoạt động
                        log_subscription_activity(
                            &tx.user_id,
                            format!("Cập nhật trạng thái giao dịch {:?}: {:?}", tx.tx_hash, new_status),
                        );
                    }
                },
                Err(e) => {
                    error!("Lỗi khi kiểm tra giao dịch {}: {:?}", tx.tx_hash, e);
                }
            }
        }
        
        Ok(())
    }

    /// Kiểm tra trạng thái giao dịch
    async fn check_transaction_status(&self, tx: &PaymentTransaction) -> Result<TransactionStatus, WalletError> {
        let tx_hash_str = format!("{:?}", tx.tx_hash);
        
        // Kiểm tra số lần retry
        if tx.retry_count > 10 {
            warn!("Vượt quá số lần kiểm tra cho giao dịch {}: {} lần", tx_hash_str, tx.retry_count);
            return Ok(TransactionStatus::Expired);
        }
        
        // Tạo timeout cho toàn bộ quá trình kiểm tra
        let timeout_duration = tokio::time::Duration::from_secs(15); // 15 giây timeout
        
        // Sử dụng tokio::time::timeout để đảm bảo không bị treo khi gọi blockchain API
        match tokio::time::timeout(
            timeout_duration, 
            self.check_transaction_once(tx)
        ).await {
            Ok(result) => result,
            Err(_) => {
                // Timeout khi thực hiện toàn bộ quá trình
                warn!("Timeout khi kiểm tra giao dịch {}", tx_hash_str);
                Ok(tx.status) // Giữ nguyên trạng thái cũ để thử lại sau
            }
        }
    }
    
    /// Kiểm tra trạng thái giao dịch một lần
    async fn check_transaction_once(&self, tx: &PaymentTransaction) -> Result<TransactionStatus, WalletError> {
        let tx_hash_str = format!("{:?}", tx.tx_hash);
        debug!("Kiểm tra trạng thái giao dịch {}", tx_hash_str);
        
        // Thực hiện truy vấn blockchain với cơ chế retry và xử lý lỗi chi tiết
        const MAX_RETRIES: u8 = 3;
        let mut current_retry = 0;
        let mut last_error: Option<WalletError> = None;
        
        while current_retry < MAX_RETRIES {
            // Nếu không phải lần đầu tiên, thêm delay
            if current_retry > 0 {
                // Thêm exponential backoff với jitter
                let backoff_ms = 100 * u64::pow(2, current_retry as u32);
                let jitter = rand::random::<u64>() % 100;
                let delay = backoff_ms + jitter;
                
                debug!("Thử lại lần {}/{} để kiểm tra giao dịch {} sau {}ms", 
                     current_retry + 1, MAX_RETRIES, tx_hash_str, delay);
                tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
            }
            
            // Kiểm tra transaction trên blockchain
            match self.wallet_manager.get_transaction_receipt(&tx_hash_str).await {
                Ok(Some(receipt)) => {
                    info!("Tìm thấy receipt cho giao dịch {}", tx_hash_str);
                    
                    // Kiểm tra trạng thái
                    if receipt.status.unwrap_or_default().as_u64() == 1 {
                        // Giao dịch thành công, xác minh số tiền
                        match self.verify_payment_amount(tx).await {
                            Ok(true) => {
                                info!("Giao dịch {} đã được xác nhận và số tiền đã được xác minh", tx_hash_str);
                                return Ok(TransactionStatus::Confirmed);
                            },
                            Ok(false) => {
                                warn!("Giao dịch {} thành công nhưng số tiền không đúng", tx_hash_str);
                                return Ok(TransactionStatus::Failed);
                            },
                            Err(e) => {
                                error!("Lỗi khi xác minh số tiền cho giao dịch {}: {}", tx_hash_str, e);
                                last_error = Some(e);
                                
                                // Kiểm tra xem lỗi có thể retry không
                                match &e {
                                    WalletError::NetworkError(_) | WalletError::TimeoutError(_) => {
                                        current_retry += 1;
                                        continue;
                                    },
                                    WalletError::BlockchainQueryError(ref msg) => {
                                        error!("Lỗi truy vấn blockchain khi lấy số tiền giao dịch {}: {}", tx_hash_str, msg);
                                        return Err(WalletError::BlockchainQueryError(format!(
                                            "Lỗi truy vấn blockchain khi lấy số tiền: {}", msg
                                        )));
                                    },
                                    WalletError::RpcError(ref msg) => {
                                        error!("Lỗi RPC khi lấy số tiền giao dịch {}: {}", tx_hash_str, msg);
                                        return Err(WalletError::RpcError(format!(
                                            "Lỗi RPC khi lấy số tiền giao dịch: {}", msg
                                        )));
                                    },
                                    _ => return Err(e)
                                }
                            }
                        }
                    } else {
                        // Giao dịch thất bại
                        warn!("Giao dịch {} thất bại trên blockchain", tx_hash_str);
                        
                        // Lấy lý do thất bại
                        match self.wallet_manager.get_transaction_failure_reason(&tx_hash_str).await {
                            Ok(reason) => {
                                warn!("Lý do thất bại cho giao dịch {}: {}", tx_hash_str, reason);
                            },
                            Err(e) => {
                                error!("Không thể lấy lý do thất bại cho giao dịch {}: {}", tx_hash_str, e);
                            }
                        }
                        
                        return Ok(TransactionStatus::Failed);
                    }
                },
                Ok(None) => {
                    // Giao dịch không tìm thấy
                    debug!("Giao dịch {} không tìm thấy trên blockchain (lần thử {}/{})", 
                         tx_hash_str, current_retry + 1, MAX_RETRIES);
                    
                    // Kiểm tra thời gian tồn tại của giao dịch
                    let transaction_age = Utc::now().signed_duration_since(tx.timestamp);
                    if transaction_age > chrono::Duration::hours(24) {
                        warn!("Giao dịch {} quá cũ (> 24 giờ), đánh dấu là hết hạn", tx_hash_str);
                        return Ok(TransactionStatus::Expired);
                    }
                    
                    // Nếu chưa hết số lần thử và giao dịch không quá cũ, tiếp tục thử
                    current_retry += 1;
                    if current_retry >= MAX_RETRIES {
                        // Nếu đã hết số lần thử và vẫn không tìm thấy, giữ nguyên trạng thái pending
                        debug!("Đã thử {} lần nhưng không tìm thấy giao dịch {}, giữ nguyên trạng thái", 
                             MAX_RETRIES, tx_hash_str);
                        return Ok(tx.status);
                    }
                },
                Err(e) => {
                    // Xử lý lỗi
                    error!("Lỗi khi kiểm tra giao dịch {} (lần thử {}/{}): {}", 
                          tx_hash_str, current_retry + 1, MAX_RETRIES, e);
                    
                    last_error = Some(e.clone());
                    
                    // Kiểm tra xem lỗi có thể retry không
                    match &e {
                        WalletError::NetworkError(_) | WalletError::TimeoutError(_) => {
                            current_retry += 1;
                            if current_retry >= MAX_RETRIES {
                                warn!("Đã thử {} lần nhưng vẫn gặp lỗi khi kiểm tra giao dịch {}, giữ nguyên trạng thái", 
                                     MAX_RETRIES, tx_hash_str);
                                return Ok(tx.status);
                            }
                        },
                        WalletError::BlockchainQueryError(ref msg) => {
                            error!("Lỗi truy vấn blockchain khi lấy số tiền giao dịch {}: {}", tx_hash_str, msg);
                            return Err(WalletError::BlockchainQueryError(format!(
                                "Lỗi truy vấn blockchain khi lấy số tiền: {}", msg
                            )));
                        },
                        WalletError::RpcError(ref msg) => {
                            error!("Lỗi RPC khi lấy số tiền giao dịch {}: {}", tx_hash_str, msg);
                            return Err(WalletError::RpcError(format!(
                                "Lỗi RPC khi lấy số tiền giao dịch: {}", msg
                            )));
                        },
                        _ => return Err(e)
                    }
                }
            }
        }
        
        // Nếu đến đây có nghĩa là đã hết số lần thử nhưng không có kết quả cuối cùng
        if let Some(e) = last_error {
            warn!("Không thể xác định trạng thái giao dịch {} sau {} lần thử: {}", 
                 tx_hash_str, MAX_RETRIES, e);
            
            // Giữ nguyên trạng thái để thử lại sau
            Ok(tx.status)
        } else {
            // Không có lỗi nhưng cũng không có kết quả rõ ràng
            warn!("Không thể xác định trạng thái giao dịch {} sau {} lần thử", 
                 tx_hash_str, MAX_RETRIES);
            
            // Giữ nguyên trạng thái để thử lại sau
            Ok(tx.status)
        }
    }

    /// Xác minh số tiền thanh toán từ blockchain
    async fn verify_payment_amount(&self, tx: &PaymentTransaction) -> Result<bool, WalletError> {
        let tx_hash_str = format!("{:?}", tx.tx_hash);
        debug!("Bắt đầu xác minh số tiền thanh toán cho giao dịch: {}", tx_hash_str);
        
        // Kiểm tra tham số đầu vào
        if tx.user_id.is_empty() {
            error!("user_id không hợp lệ khi xác minh số tiền: {:?}", tx.tx_hash);
            return Err(WalletError::InvalidParameter("user_id không được để trống".to_string()));
        }
        
        if tx.amount.is_zero() {
            error!("Số tiền dự kiến không hợp lệ: {:?}", tx.tx_hash);
            return Err(WalletError::InvalidParameter("Số tiền thanh toán dự kiến không hợp lệ".to_string()));
        }
        
        // Kiểm tra thời gian tồn tại của giao dịch
        let transaction_age = Utc::now().signed_duration_since(tx.timestamp);
        if transaction_age > chrono::Duration::hours(72) {
            warn!("Giao dịch {} quá cũ (> 72 giờ), có thể gặp vấn đề khi xác minh", tx_hash_str);
            
            // Ghi log chi tiết về việc giao dịch quá cũ
            warn!(
                "Giao dịch của user {} quá cũ (> 72 giờ): {} - Thời gian: {}",
                tx.user_id, tx_hash_str, tx.timestamp
            );
        }
        
        // Kiểm tra và lấy số tiền thực tế từ blockchain
        let actual_amount = match self.wallet_manager.get_transaction_amount(&tx_hash_str).await {
            Ok(amount) => amount,
            Err(e) => {
                error!("Lỗi khi lấy số tiền giao dịch {} từ blockchain: {:?}", tx_hash_str, e);
                
                match e {
                    WalletError::NetworkError(ref msg) => {
                        error!("Lỗi mạng khi lấy số tiền giao dịch {}: {}", tx_hash_str, msg);
                        return Err(WalletError::NetworkError(format!(
                            "Lỗi mạng khi lấy số tiền giao dịch: {}", msg
                        )));
                    },
                    WalletError::TimeoutError(ref msg) => {
                        warn!("Timeout khi lấy số tiền giao dịch {}: {}", tx_hash_str, msg);
                        return Err(WalletError::TimeoutError(format!(
                            "Timeout khi lấy số tiền giao dịch: {}", msg
                        )));
                    },
                    WalletError::BlockchainQueryError(ref msg) => {
                        error!("Lỗi truy vấn blockchain khi lấy số tiền giao dịch {}: {}", tx_hash_str, msg);
                        return Err(WalletError::BlockchainQueryError(format!(
                            "Lỗi truy vấn blockchain khi lấy số tiền: {}", msg
                        )));
                    },
                    WalletError::RpcError(ref msg) => {
                        error!("Lỗi RPC khi lấy số tiền giao dịch {}: {}", tx_hash_str, msg);
                        return Err(WalletError::RpcError(format!(
                            "Lỗi RPC khi lấy số tiền giao dịch: {}", msg
                        )));
                    },
                    _ => return Err(e),
                }
            }
        };
        
        // So sánh số tiền thực tế với số tiền dự kiến
        if actual_amount < tx.amount {
            warn!(
                "Số tiền thanh toán không đủ cho giao dịch {}: Yêu cầu {} nhưng chỉ nhận được {}",
                tx.tx_hash, tx.amount, actual_amount
            );
            
            // Ghi log chi tiết về số tiền không đủ
            error!(
                "Chi tiết thanh toán không đủ - User: {}, Subscription: {:?}, Token: {:?}, Expected: {}, Received: {}",
                tx.user_id, tx.subscription_type, tx.payment_token, tx.amount, actual_amount
            );
            
            // Kiểm tra xem số tiền có đủ để nâng cấp lên gói thấp hơn không
            if tx.subscription_type == SubscriptionType::Vip && 
               actual_amount >= calculate_payment_amount(SubscriptionType::Premium, tx.payment_token) {
                warn!(
                    "Số tiền đủ để nâng cấp lên gói Premium cho user {} thay vì VIP: {} - Nhận: {}",
                    tx.user_id, tx.amount, actual_amount
                );
                
                // TODO: Xử lý logic nâng cấp lên gói thấp hơn dựa trên số tiền thực tế
                // Trả về true nhưng xử lý ở lớp cao hơn
                info!(
                    "Chấp nhận thanh toán với số tiền thấp hơn, điều chỉnh gói đăng ký cho user {}",
                    tx.user_id
                );
                return Ok(true);
            }
            
            return Ok(false);
        }
        
        // Kiểm tra loại token thanh toán
        let token_info = match self.wallet_manager.get_token_info_from_tx(&tx_hash_str).await {
            Ok(info) => info,
            Err(e) => {
                error!("Lỗi khi lấy thông tin token từ giao dịch {}: {:?}", tx_hash_str, e);
                
                match e {
                    WalletError::NetworkError(ref msg) => {
                        error!("Lỗi mạng khi lấy thông tin token từ giao dịch {}: {}", tx_hash_str, msg);
                        return Err(WalletError::NetworkError(format!(
                            "Lỗi mạng khi lấy thông tin token: {}", msg
                        )));
                    },
                    WalletError::TimeoutError(ref msg) => {
                        warn!("Timeout khi lấy thông tin token từ giao dịch {}: {}", tx_hash_str, msg);
                        return Err(WalletError::TimeoutError(format!(
                            "Timeout khi lấy thông tin token: {}", msg
                        )));
                    },
                    WalletError::BlockchainQueryError(ref msg) => {
                        error!("Lỗi truy vấn blockchain khi lấy thông tin token từ giao dịch {}: {}", tx_hash_str, msg);
                        return Err(WalletError::BlockchainQueryError(format!(
                            "Lỗi truy vấn blockchain khi lấy thông tin token: {}", msg
                        )));
                    },
                    WalletError::RpcError(ref msg) => {
                        error!("Lỗi RPC khi lấy thông tin token từ giao dịch {}: {}", tx_hash_str, msg);
                        return Err(WalletError::RpcError(format!(
                            "Lỗi RPC khi lấy thông tin token: {}", msg
                        )));
                    },
                    _ => return Err(e),
                }
            }
        };
        
        // Kiểm tra xem đúng loại token không
        let expected_token_address = self.wallet_manager.get_token_address(tx.payment_token);
        if token_info.address != expected_token_address {
            error!(
                "Loại token không đúng cho giao dịch {}: Expected {} nhưng nhận được {}",
                tx_hash_str, expected_token_address, token_info.address
            );
            
            // Ghi log chi tiết về token không đúng
            error!(
                "Chi tiết token không đúng - User: {}, Expected: {}, Received: {}, Symbol: {}",
                tx.user_id, expected_token_address, token_info.address, token_info.symbol
            );
            
            return Ok(false);
        }
        
        // Số tiền đủ
        debug!("Xác minh thành công số tiền thanh toán cho giao dịch {}", tx_hash_str);
        
        // Ghi log thành công
        info!(
            "Xác minh thành công thanh toán - User: {}, Hash: {}, Subscription: {:?}, Amount: {}",
            tx.user_id, tx_hash_str, tx.subscription_type, tx.amount
        );
        
        Ok(true)
    }

    /// Cập nhật trạng thái giao dịch
    async fn update_transaction_status(
        &self, 
        tx_hash: &H256, 
        new_status: TransactionStatus,
        retry_count: u8,
    ) {
        let mut transactions = self.pending_transactions.write().await;
        
        if let Some(tx) = transactions.iter_mut().find(|t| t.tx_hash == *tx_hash) {
            tx.status = new_status;
            tx.retry_count = retry_count;
        }
    }

    /// Lấy thông tin giao dịch theo hash
    pub async fn get_transaction(&self, tx_hash: &H256) -> Option<PaymentTransaction> {
        let transactions = self.pending_transactions.read().await;
        transactions.iter().find(|tx| tx.tx_hash == *tx_hash).cloned()
    }

    /// Kiểm tra xem giao dịch đã được xác nhận chưa
    pub async fn is_transaction_confirmed(&self, user_id: &str, tx_hash: &H256) -> bool {
        let transactions = self.pending_transactions.read().await;
        
        transactions
            .iter()
            .any(|tx| tx.user_id == user_id && tx.tx_hash == *tx_hash && tx.status == TransactionStatus::Confirmed)
    }
} 