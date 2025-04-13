//! Module quản lý đăng ký và subscription của người dùng.

// Standard library imports
use std::collections::HashMap;
use std::sync::Arc;

// External imports
use chrono::{DateTime, Duration, Utc};
use ethers::types::{Address, U256};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{info, warn, error};
use uuid::Uuid;

// Internal imports
use crate::error::WalletError;
use crate::users::free_user::{FreeUserData, FreeUserManager};
use crate::users::premium_user::{PremiumUserData, PremiumUserManager};
use crate::walletmanager::api::WalletManagerApi;
use crate::walletmanager::chain::ChainType;

use super::{
    auto_trade::AutoTradeManager,
    constants::*,
    nft::{NonNftVipStatus, NftInfo, VipNftInfo},
    types::{
        Feature, PaymentToken, SubscriptionPlan, SubscriptionStatus, 
        SubscriptionType, TransactionCheckResult, UserSubscription
    },
};

/// Quản lý đăng ký nâng cấp tài khoản.
pub struct SubscriptionManager {
    /// Danh sách gói đăng ký
    subscription_plans: Vec<SubscriptionPlan>,
    /// Thông tin đăng ký của người dùng
    user_subscriptions: Arc<RwLock<HashMap<String, UserSubscription>>>,
    /// Địa chỉ ví DMD token
    dmd_token_address: Address,
    /// Địa chỉ ví USDC token
    usdc_token_address: Address,
    /// API quản lý ví
    wallet_api: Arc<RwLock<WalletManagerApi>>,
    /// Quản lý người dùng free
    free_user_manager: Arc<FreeUserManager>,
    /// Quản lý người dùng premium
    premium_user_manager: Arc<PremiumUserManager>,
    /// Quản lý thời gian sử dụng auto_trade
    auto_trade_manager: Option<Arc<AutoTradeManager>>,
}

impl SubscriptionManager {
    /// Khởi tạo SubscriptionManager mới.
    pub fn new(
        wallet_api: Arc<RwLock<WalletManagerApi>>,
        free_user_manager: Arc<FreeUserManager>,
        premium_user_manager: Arc<PremiumUserManager>,
        dmd_token_address: Address,
        usdc_token_address: Address,
    ) -> Self {
        let subscription_plans = vec![
            SubscriptionPlan {
                plan_type: SubscriptionType::Premium,
                name: "Premium Plan".to_string(),
                description: "Gói premium với quyền sử dụng auto_trade và các tính năng nâng cao".to_string(),
                price_dmd: 100,
                price_usdc: 100,
                duration_days: 30,
                features: vec![
                    Feature::AutoTrade,
                    Feature::AdvancedAnalytics,
                    Feature::PrioritySupport,
                ],
            },
            SubscriptionPlan {
                plan_type: SubscriptionType::VIP,
                name: "VIP Plan".to_string(),
                description: "Gói VIP với toàn bộ tính năng không giới hạn".to_string(),
                price_dmd: 300,
                price_usdc: 300,
                duration_days: 30,
                features: vec![
                    Feature::AutoTrade,
                    Feature::AdvancedAnalytics,
                    Feature::PremiumSupport,
                    Feature::UnlimitedTransactions,
                    Feature::CustomStrategies,
                ],
            },
        ];

        let instance = Self {
            subscription_plans,
            user_subscriptions: Arc::new(RwLock::new(HashMap::new())),
            dmd_token_address,
            usdc_token_address,
            wallet_api,
            free_user_manager,
            premium_user_manager,
            auto_trade_manager: None,
        };
        
        instance
    }
    
    /// Khởi tạo AutoTradeManager.
    /// Gọi hàm này sau khi đã tạo SubscriptionManager để tránh circular reference.
    ///
    /// # Lưu ý về Thread Safety
    /// Cần đảm bảo rằng instance này không được sử dụng song song với AutoTradeManager
    /// để tránh deadlock.
    pub fn init_auto_trade_manager(&mut self) {
        // Tạo một bản copy của instance hiện tại mà không có auto_trade_manager
        // để tránh circular reference
        let subscription_manager = Arc::new(Self {
            subscription_plans: self.subscription_plans.clone(),
            user_subscriptions: self.user_subscriptions.clone(),
            dmd_token_address: self.dmd_token_address,
            usdc_token_address: self.usdc_token_address,
            wallet_api: self.wallet_api.clone(),
            free_user_manager: self.free_user_manager.clone(),
            premium_user_manager: self.premium_user_manager.clone(),
            auto_trade_manager: None,
        });
        
        // Tạo mới AutoTradeManager sử dụng bản copy
        let auto_trade_manager = Arc::new(AutoTradeManager::new(subscription_manager));
        self.auto_trade_manager = Some(auto_trade_manager);
    }
    
    /// Lấy AutoTradeManager.
    pub fn get_auto_trade_manager(&self) -> Result<Arc<AutoTradeManager>, WalletError> {
        self.auto_trade_manager
            .clone()
            .ok_or(WalletError::Other("AutoTradeManager chưa được khởi tạo".to_string()))
    }
    
    /// Clone instance mà không bao gồm auto_trade_manager để tránh circular reference.
    pub fn clone(&self) -> Self {
        Self {
            subscription_plans: self.subscription_plans.clone(),
            user_subscriptions: self.user_subscriptions.clone(),
            dmd_token_address: self.dmd_token_address,
            usdc_token_address: self.usdc_token_address,
            wallet_api: self.wallet_api.clone(),
            free_user_manager: self.free_user_manager.clone(),
            premium_user_manager: self.premium_user_manager.clone(),
            auto_trade_manager: None,
        }
    }

    /// Lấy danh sách gói đăng ký.
    pub fn get_subscription_plans(&self) -> &Vec<SubscriptionPlan> {
        &self.subscription_plans
    }

    /// Kiểm tra nếu người dùng đã có quyền sử dụng một tính năng.
    ///
    /// # Arguments
    /// - `user_id`: ID người dùng.
    /// - `feature`: Tính năng cần kiểm tra.
    ///
    /// # Returns
    /// - `Ok(true)`: Nếu người dùng có quyền sử dụng tính năng.
    /// - `Ok(false)`: Nếu người dùng không có quyền sử dụng tính năng.
    /// - `Err`: Nếu có lỗi.
    #[flow_from("snipebot::auto_trade")]
    pub async fn can_use_feature(&self, user_id: &str, feature: Feature) -> Result<bool, WalletError>
    where
        Feature: Send + Sync + 'static,
    {
        let subscriptions = self.user_subscriptions.read().await;
        
        if let Some(subscription) = subscriptions.get(user_id) {
            // Kiểm tra nếu đăng ký đã hết hạn
            if subscription.end_date < Utc::now() {
                return Ok(false);
            }
            
            // Kiểm tra trạng thái đăng ký
            if subscription.status != SubscriptionStatus::Active {
                return Ok(false);
            }
            
            // Lấy thông tin gói đăng ký
            let plan = self.subscription_plans.iter()
                .find(|p| p.plan_type == subscription.subscription_type)
                .ok_or(WalletError::Other("Không tìm thấy thông tin gói đăng ký".to_string()))?;
            
            // Kiểm tra nếu gói đăng ký có tính năng được yêu cầu
            return Ok(plan.features.contains(&feature));
        }
        
        // Nếu không tìm thấy đăng ký, người dùng là free_user và không có quyền cao cấp
        Ok(false)
    }

    /// Kiểm tra nếu người dùng có thể sử dụng chức năng auto_trade.
    ///
    /// # Arguments
    /// - `user_id`: ID người dùng.
    ///
    /// # Returns
    /// - `Ok(true)`: Nếu người dùng có thể sử dụng auto_trade.
    /// - `Ok(false)`: Nếu người dùng không thể sử dụng auto_trade.
    /// - `Err`: Nếu có lỗi.
    #[flow_from("snipebot::auto_trade")]
    pub async fn can_use_auto_trade(&self, user_id: &str) -> Result<bool, WalletError> {
        self.can_use_feature(user_id, Feature::AutoTrade).await
    }

    /// Tạo đăng ký mới.
    ///
    /// # Arguments
    /// - `user_id`: ID người dùng.
    /// - `subscription_type`: Loại gói đăng ký.
    /// - `payment_address`: Địa chỉ ví dùng để thanh toán.
    /// - `payment_token`: Token sử dụng để thanh toán (DMD hoặc USDC).
    ///
    /// # Returns
    /// - `Ok(UserSubscription)`: Thông tin đăng ký mới.
    /// - `Err`: Nếu có lỗi.
    #[flow_from("wallet::ui")]
    pub async fn create_subscription(
        &self,
        user_id: &str,
        subscription_type: SubscriptionType,
        payment_address: Address,
        payment_token: PaymentToken,
    ) -> Result<UserSubscription, WalletError> {
        // Kiểm tra người dùng hiện tại
        let free_user_exists = self.free_user_manager.get_user_data(user_id).await.is_ok();
        
        if !free_user_exists {
            return Err(WalletError::Other("Người dùng không tồn tại".to_string()));
        }
        
        // Tìm gói đăng ký
        let plan = self.subscription_plans.iter()
            .find(|p| p.plan_type == subscription_type)
            .ok_or(WalletError::Other("Không tìm thấy gói đăng ký yêu cầu".to_string()))?;
        
        // Xác định giá trị thanh toán
        let payment_amount = match payment_token {
            PaymentToken::DMD => U256::from(plan.price_dmd),
            PaymentToken::USDC => U256::from(plan.price_usdc),
        };
        
        let now = Utc::now();
        let end_date = now + Duration::days(plan.duration_days as i64);
        
        let subscription = UserSubscription {
            user_id: user_id.to_string(),
            subscription_type,
            start_date: now,
            end_date,
            payment_address,
            payment_tx_hash: None,
            payment_token,
            payment_amount,
            status: SubscriptionStatus::Pending,
            nft_info: None,
            non_nft_status: NonNftVipStatus::default(),
        };
        
        // Lưu thông tin đăng ký
        let mut subscriptions = self.user_subscriptions.write().await;
        subscriptions.insert(user_id.to_string(), subscription.clone());
        
        info!("Tạo đăng ký mới cho người dùng {}: {:?}", user_id, subscription);
        
        Ok(subscription)
    }

    /// Kiểm tra trạng thái của giao dịch trên blockchain.
    ///
    /// # Arguments
    /// - `tx_hash`: Mã giao dịch cần kiểm tra.
    /// - `chain_id`: ID của chain.
    ///
    /// # Returns
    /// - `Ok(TransactionCheckResult)`: Kết quả kiểm tra giao dịch.
    /// - `Err`: Nếu có lỗi.
    #[flow_from("blockchain::transactions")]
    async fn check_transaction_status(
        &self,
        tx_hash: &str,
        chain_id: u64,
    ) -> Result<TransactionCheckResult, WalletError> {
        // Thực hiện kiểm tra với số lần thử quy định
        for attempt in 0..BLOCKCHAIN_TX_RETRY_ATTEMPTS {
            match self.check_transaction_status_internal(tx_hash, chain_id).await {
                Ok(result) => {
                    // Nếu giao dịch đang pending và chưa phải lần thử cuối, tiếp tục đợi
                    if result == TransactionCheckResult::Pending && attempt < BLOCKCHAIN_TX_RETRY_ATTEMPTS - 1 {
                        tokio::time::sleep(tokio::time::Duration::from_millis(BLOCKCHAIN_TX_RETRY_DELAY_MS)).await;
                        continue;
                    }
                    return Ok(result);
                },
                Err(e) if attempt < BLOCKCHAIN_TX_RETRY_ATTEMPTS - 1 => {
                    // Nếu có lỗi và chưa phải lần thử cuối, thử lại
                    warn!("Lỗi khi kiểm tra giao dịch {}, lần thử {}/{}: {}", 
                        tx_hash, attempt + 1, BLOCKCHAIN_TX_RETRY_ATTEMPTS, e);
                    tokio::time::sleep(tokio::time::Duration::from_millis(BLOCKCHAIN_TX_RETRY_DELAY_MS)).await;
                },
                Err(e) => return Err(e),
            }
        }
        
        // Nếu đã thử đủ số lần mà vẫn không có kết quả rõ ràng, trả về NotFound
        Ok(TransactionCheckResult::NotFound)
    }
    
    /// Kiểm tra giao dịch từ blockchain (internal).
    async fn check_transaction_status_internal(
        &self, 
        tx_hash: &str, 
        chain_id: u64
    ) -> Result<TransactionCheckResult, WalletError> {
        let wallet_api = self.wallet_api.read().await;
        
        // TODO: Triển khai thực tế để kiểm tra giao dịch trên blockchain
        // Đây là phiên bản đơn giản hóa, trong thực tế cần kết nối với node blockchain
        
        // Giả lập kiểm tra giao dịch thành công cho demo
        if tx_hash.len() == 66 && tx_hash.starts_with("0x") {
            return Ok(TransactionCheckResult::Confirmed);
        } else if tx_hash.len() == 66 && tx_hash.starts_with("0xp") {
            return Ok(TransactionCheckResult::Pending);
        } else if tx_hash.len() == 66 && tx_hash.starts_with("0xf") {
            return Ok(TransactionCheckResult::Failed("Giao dịch bị từ chối".to_string()));
        }
        
        Ok(TransactionCheckResult::NotFound)
    }

    /// Xử lý thanh toán đăng ký.
    ///
    /// # Arguments
    /// - `user_id`: ID người dùng.
    /// - `tx_hash`: Mã giao dịch thanh toán.
    /// - `chain_id`: ID của chain thực hiện giao dịch.
    ///
    /// # Returns
    /// - `Ok(())`: Nếu xử lý thanh toán thành công.
    /// - `Err`: Nếu có lỗi.
    #[flow_from("wallet::ui")]
    pub async fn process_payment(
        &self,
        user_id: &str,
        tx_hash: &str,
        chain_id: u64,
    ) -> Result<(), WalletError> {
        let mut subscriptions = self.user_subscriptions.write().await;
        
        let subscription = subscriptions.get_mut(user_id)
            .ok_or(WalletError::Other("Không tìm thấy thông tin đăng ký".to_string()))?;
        
        if subscription.status != SubscriptionStatus::Pending {
            return Err(WalletError::Other("Trạng thái đăng ký không hợp lệ".to_string()));
        }
        
        // Kiểm tra giao dịch trên blockchain
        let tx_status = self.check_transaction_status(tx_hash, chain_id).await?;
        
        match tx_status {
            TransactionCheckResult::Confirmed => {
                // Cập nhật thông tin đăng ký
                subscription.payment_tx_hash = Some(tx_hash.to_string());
                subscription.status = SubscriptionStatus::Active;
                
                info!("Đã xử lý thanh toán cho đăng ký của người dùng {}: tx_hash={}", user_id, tx_hash);
                
                // Nâng cấp tài khoản người dùng
                drop(subscriptions); // Tránh deadlock khi gọi upgrade_user_account
                self.upgrade_user_account(user_id, subscription.subscription_type).await?;
                
                Ok(())
            },
            TransactionCheckResult::Pending => {
                Err(WalletError::Other("Giao dịch đang chờ xử lý, vui lòng thử lại sau".to_string()))
            },
            TransactionCheckResult::Failed(reason) => {
                subscription.status = SubscriptionStatus::Error;
                Err(WalletError::Other(format!("Giao dịch thất bại: {}", reason)))
            },
            TransactionCheckResult::NotFound => {
                Err(WalletError::Other("Không tìm thấy giao dịch, vui lòng kiểm tra lại mã giao dịch".to_string()))
            },
        }
    }

    /// Kiểm tra xem NFT có tồn tại trong ví của user không
    ///
    /// # Arguments
    /// - `wallet_address`: Địa chỉ ví cần kiểm tra
    /// - `nft_contract`: Địa chỉ smart contract của NFT
    /// - `token_id`: ID của token (chuẩn ERC-1155)
    /// - `chain_id`: ID của chain chứa NFT
    ///
    /// # Returns
    /// - `Ok(true)`: Nếu NFT tồn tại trong ví
    /// - `Ok(false)`: Nếu NFT không tồn tại trong ví
    /// - `Err`: Nếu có lỗi khi kiểm tra
    #[flow_from("wallet::nft")]
    pub async fn check_nft_ownership(
        &self,
        wallet_address: Address,
        nft_contract: Address,
        token_id: U256,
        chain_id: u64,
    ) -> Result<bool, WalletError> {
        let wallet_api = self.wallet_api.read().await;
        
        // TODO: Triển khai thực tế để kiểm tra NFT trên blockchain
        // Đây là ví dụ đơn giản, cần kết nối đến blockchain thực tế để kiểm tra balance của NFT
        
        // Ví dụ mock:
        if wallet_address.is_zero() || nft_contract.is_zero() {
            return Ok(false);
        }
        
        // Trong thực tế, cần gọi hàm balanceOf từ smart contract ERC-1155
        // Và kiểm tra nếu balance > 0
        
        // Giả lập trả về true
        Ok(true)
    }

    // Thêm các phương thức khác liên quan đến quản lý subscription
    // ...
} 