//! Module quản lý thanh toán cho các gói đăng ký.

use chrono::{DateTime, Utc};
use ethers::types::{Address, H256, TransactionReceipt, U256};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, error};

use crate::blockchain::types::TokenInfo;
use crate::error::AppResult;
use crate::walletmanager::errors::WalletError;
use crate::walletmanager::WalletManagerApi;

use super::constants::{
    BLOCKCHAIN_TX_CONFIRMATION_BLOCKS, 
    BLOCKCHAIN_TX_RETRY_ATTEMPTS, 
    BLOCKCHAIN_TX_RETRY_DELAY_MS
};
use super::types::{PaymentToken, SubscriptionType, TransactionCheckResult};
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
    
    /// Kiểm tra trạng thái giao dịch trên blockchain
    pub async fn check_transaction_status(
        &self,
        tx_hash: &str,
    ) -> AppResult<TransactionCheckResult> {
        // Thực hiện kiểm tra với số lần thử quy định
        for attempt in 0..BLOCKCHAIN_TX_RETRY_ATTEMPTS {
            match self.check_transaction_once(tx_hash).await {
                Ok(result) => {
                    // Nếu giao dịch đang pending và chưa phải lần thử cuối, tiếp tục đợi
                    if matches!(result, TransactionCheckResult::Pending) && 
                       attempt < BLOCKCHAIN_TX_RETRY_ATTEMPTS - 1 {
                        tokio::time::sleep(tokio::time::Duration::from_millis(BLOCKCHAIN_TX_RETRY_DELAY_MS)).await;
                        continue;
                    }
                    return Ok(result);
                },
                Err(e) if attempt < BLOCKCHAIN_TX_RETRY_ATTEMPTS - 1 => {
                    // Thử lại nếu có lỗi và chưa phải lần cuối
                    tokio::time::sleep(tokio::time::Duration::from_millis(BLOCKCHAIN_TX_RETRY_DELAY_MS)).await;
                },
                Err(e) => return Err(e.into()),
            }
        }
        
        // Nếu đã thử đủ số lần mà vẫn không có kết quả, trả về Timeout
        Ok(TransactionCheckResult::Timeout)
    }
    
    /// Kiểm tra giao dịch một lần
    async fn check_transaction_once(&self, tx_hash: &str) -> Result<TransactionCheckResult, WalletError> {
        // TODO: Triển khai chi tiết kết nối blockchain và kiểm tra giao dịch
        // Đây là code mẫu để giả lập việc kiểm tra
        
        // Giả lập các kết quả khác nhau dựa trên hash giao dịch (cho mục đích demo)
        if tx_hash.starts_with("0xc") {
            // Đã xác nhận
            let token_info = TokenInfo {
                address: self.usdc_token_address,
                name: "USDC".to_string(),
                symbol: "USDC".to_string(),
                decimals: 6,
            };
            
            return Ok(TransactionCheckResult::Confirmed {
                token: token_info,
                amount: U256::from(100), // Ví dụ: 100 USDC
                plan: SubscriptionType::Premium,
                tx_id: tx_hash.to_string(),
            });
        } else if tx_hash.starts_with("0xp") {
            // Đang chờ xử lý
            return Ok(TransactionCheckResult::Pending);
        } else if tx_hash.starts_with("0xf") {
            // Thất bại
            return Ok(TransactionCheckResult::Failed {
                reason: "Giao dịch bị từ chối bởi mạng".to_string(),
            });
        }
        
        // Không tìm thấy
        Ok(TransactionCheckResult::Timeout)
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

    /// Kiểm tra trạng thái của một giao dịch cụ thể
    async fn check_transaction_status(&self, tx: &PaymentTransaction) -> Result<TransactionStatus, WalletError> {
        // Kiểm tra trạng thái giao dịch trên blockchain
        // TODO: Cần triển khai chi tiết kết nối với blockchain thông qua wallet_manager
        
        // Giả lập kiểm tra trạng thái giao dịch cho mục đích demo
        let tx_hash_str = format!("{:?}", tx.tx_hash);
        
        if tx_hash_str.starts_with("0xc") {
            // Đã xác nhận
            return Ok(TransactionStatus::Confirmed);
        } else if tx_hash_str.starts_with("0xp") {
            // Vẫn đang chờ
            return Ok(TransactionStatus::Pending);
        } else if tx_hash_str.starts_with("0xf") {
            // Thất bại
            return Ok(TransactionStatus::Failed);
        } else if tx.retry_count > 10 {
            // Quá nhiều lần thử, coi như hết hạn
            return Ok(TransactionStatus::Expired);
        }
        
        // Không có thay đổi
        Ok(tx.status)
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