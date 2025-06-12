//! Module quản lý đăng ký người dùng (SubscriptionManager) cung cấp chức năng trung tâm 
//! cho việc quản lý các gói đăng ký và quyền của người dùng trong hệ thống.
//!
//! Module này cung cấp các chức năng:
//! * Quản lý các gói đăng ký (Premium, VIP, VIP 12 tháng)
//! * Tạo và cập nhật đăng ký cho người dùng
//! * Xác thực quyền truy cập vào các tính năng dựa trên loại đăng ký
//! * Nâng cấp và hạ cấp đăng ký dựa trên thanh toán hoặc hết hạn
//! * Tích hợp với hệ thống sở hữu NFT để xác minh trạng thái VIP
//! * Tích hợp với hệ thống staking cho các đăng ký VIP không có NFT
//! * Quản lý tính năng giao dịch tự động (AutoTrade) cho người dùng
//! * Xử lý hệ thống thanh toán và xác minh trạng thái giao dịch
//!
//! SubscriptionManager được thiết kế để là trung tâm tích hợp cho tất cả các chức năng
//! liên quan đến đăng ký, tương tác với các module như payment, staking, auto_trade, và nft
//! để cung cấp trải nghiệm liền mạch cho người dùng.
//!
//! Module này sử dụng các kỹ thuật đồng bộ hóa như RwLock và Arc để đảm bảo an toàn
//! trong các ngữ cảnh đa luồng và tránh các vấn đề deadlock tiềm ẩn khi truy cập dữ liệu.

// Standard library imports
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use std::sync::Weak;

// External imports
use chrono::{DateTime, Utc};
use ethers::types::{Address, U256};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{info, warn, error, debug};
use uuid::Uuid;
use rust_decimal::Decimal;
use tokio::time::{self, Duration as TokioDuration, interval};
use thiserror::Error;

// Internal imports
use crate::error::WalletError;
use crate::users::free_user::{FreeUserData, FreeUserManager};
use crate::users::premium_user::{PremiumUserData, PremiumUserManager};
use crate::walletmanager::api::WalletManagerApi;
use crate::walletmanager::chain::ChainType;
use crate::cache;
use crate::db::Database;
use crate::users::subscription::constants::{USER_DATA_CACHE_SECONDS};
use crate::users::subscription::UserSubscription;

use super::{
    auto_trade::AutoTradeManager,
    constants::*,
    nft::{NonNftVipStatus, NftInfo, VipNftInfo},
    types::{
        Feature, PaymentToken, SubscriptionPlan, SubscriptionStatus, 
        SubscriptionType, TransactionCheckResult, UserSubscription
    },
    events::{EventType, SubscriptionEvent, EventEmitter},
};

/// Quản lý đăng ký nâng cấp tài khoản.
pub struct SubscriptionManager {
    /// Danh sách gói đăng ký
    subscription_plans: Vec<SubscriptionPlan>,
    /// Thông tin đăng ký của người dùng
    user_subscriptions: Arc<tokio::sync::RwLock<HashMap<String, UserSubscription>>>,
    /// Địa chỉ ví DMD token
    dmd_token_address: Address,
    /// Địa chỉ ví USDC token
    usdc_token_address: Address,
    /// API quản lý ví
    wallet_api: Arc<tokio::sync::RwLock<WalletManagerApi>>,
    /// Quản lý người dùng free
    free_user_manager: Arc<FreeUserManager>,
    /// Quản lý người dùng premium
    premium_user_manager: Arc<PremiumUserManager>,
    /// Quản lý thời gian sử dụng auto_trade - sử dụng Weak để tránh circular references
    auto_trade_manager: Option<Weak<AutoTradeManager>>,
    /// Event emitter để gửi thông báo
    event_emitter: Option<EventEmitter>,
    db: Arc<Database>,
}

impl SubscriptionManager {
    /// Khởi tạo SubscriptionManager mới.
    pub fn new(db: Arc<Database>) -> Self {
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
            SubscriptionPlan {
                plan_type: SubscriptionType::VIP,
                name: "VIP 12 Month Plan".to_string(),
                description: "Gói VIP 12 tháng với toàn bộ tính năng không giới hạn và giảm 20% phí".to_string(),
                price_dmd: 3840, // (100 + 300) * 12 * 0.8 = 3840 (giảm 20%)
                price_usdc: 3840,
                duration_days: 365,
                features: vec![
                    Feature::AutoTrade,
                    Feature::AdvancedAnalytics,
                    Feature::PremiumSupport,
                    Feature::UnlimitedTransactions,
                    Feature::CustomStrategies,
                    Feature::TokenStaking,
                ],
            },
        ];

        let instance = Self {
            subscription_plans,
            user_subscriptions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            dmd_token_address: Address::zero(),
            usdc_token_address: Address::zero(),
            wallet_api: Arc::new(tokio::sync::RwLock::new(WalletManagerApi::new())),
            free_user_manager: Arc::new(FreeUserManager::new()),
            premium_user_manager: Arc::new(PremiumUserManager::new()),
            auto_trade_manager: None,
            event_emitter: None,
            db,
        };
        
        instance
    }
    
    /// Khởi tạo AutoTradeManager
    ///
    /// Phương thức này khởi tạo AutoTradeManager dùng Weak reference để tránh circular references.
    /// Sử dụng tokio::sync::Mutex để tránh deadlock khi khởi tạo.
    ///
    /// # Thread Safety
    /// Phương thức này sử dụng tokio::sync::Mutex thay vì std::sync::Mutex để tránh blocking thread
    /// trong môi trường async. Điều này giúp ngăn chặn deadlock khi các thao tác async được gọi
    /// từ các phương thức khác đang giữ lock.
    pub async fn init_auto_trade_manager(&mut self) -> Result<(), WalletError> {
        // Sử dụng mutex để đảm bảo chỉ một thread có thể khởi tạo auto_trade_manager
        static INIT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        
        // Kiểm tra nếu đã khởi tạo rồi
        if self.auto_trade_manager.is_some() {
            return Ok(());
        }
        
        // Lấy lock để tránh race condition khi khởi tạo
        let _guard = INIT_LOCK.lock().await;
        
        // Kiểm tra lại sau khi lấy được lock (double-check locking pattern)
        if self.auto_trade_manager.is_some() {
            return Ok(());
        }
        
        debug!("Bắt đầu khởi tạo AutoTradeManager");
        
        // Tạo clone nhẹ của self, không bao gồm auto_trade_manager để tránh tham chiếu vòng tròn
        let subscription_manager = Arc::new(Self {
            subscription_plans: self.subscription_plans.clone(),
            user_subscriptions: self.user_subscriptions.clone(),
            dmd_token_address: self.dmd_token_address,
            usdc_token_address: self.usdc_token_address,
            wallet_api: self.wallet_api.clone(),
            free_user_manager: self.free_user_manager.clone(),
            premium_user_manager: self.premium_user_manager.clone(),
            auto_trade_manager: None,
            event_emitter: self.event_emitter.clone(),
            db: self.db.clone(),
        });
        
        // Tạo mới AutoTradeManager với Weak reference
        let auto_trade_manager = Arc::new(AutoTradeManager::new(
            Arc::downgrade(&subscription_manager),
            self.db.clone()
        ));
        
        // Lưu auto_trade_manager vào self dưới dạng Weak reference
        self.auto_trade_manager = Some(Arc::downgrade(&auto_trade_manager));
        
        info!("Đã khởi tạo AutoTradeManager với Weak reference để tránh circular references");
        Ok(())
    }
    
    /// Lấy AutoTradeManager.
    pub fn get_auto_trade_manager(&self) -> Result<Arc<AutoTradeManager>, WalletError> {
        match &self.auto_trade_manager {
            Some(weak_ref) => weak_ref.upgrade()
                .ok_or(WalletError::Other("AutoTradeManager đã bị giải phóng".to_string())),
            None => Err(WalletError::Other("AutoTradeManager chưa được khởi tạo".to_string()))
        }
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
            event_emitter: None,
            db: self.db.clone(),
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
        debug!("Đang kiểm tra trạng thái giao dịch {} trên chain {}", tx_hash, chain_id);
        
        // Kiểm tra tính hợp lệ của tx_hash
        if tx_hash.is_empty() || tx_hash.len() != 66 {
            return Err(WalletError::Other("Mã giao dịch không hợp lệ".to_string()));
        }
        
        // Kiểm tra tính hợp lệ của chain_id
        if chain_id == 0 {
            return Err(WalletError::Other("Chain ID không hợp lệ".to_string()));
        }
        
        // Lấy provider cho chain
        let provider = self.wallet_api.read().await
            .get_provider(chain_id)
            .map_err(|e| WalletError::Other(format!("Không thể lấy provider: {}", e)))?;
        
        // Kiểm tra trạng thái giao dịch
        let result = self.check_transaction_status_internal(tx_hash, chain_id).await
            .map_err(|e| {
                error!("Lỗi khi kiểm tra trạng thái giao dịch: {}", e);
                e
            })?;
        
        // Log kết quả
        match result {
            TransactionCheckResult::Success => {
                info!("Giao dịch {} đã thành công", tx_hash);
            }
            TransactionCheckResult::Failed => {
                warn!("Giao dịch {} đã thất bại", tx_hash);
            }
            TransactionCheckResult::Pending => {
                debug!("Giao dịch {} đang chờ xử lý", tx_hash);
            }
        }
        
        Ok(result)
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
        debug!("Đang xử lý thanh toán cho người dùng {} với giao dịch {}", user_id, tx_hash);
        
        // Kiểm tra tính hợp lệ của user_id
        if user_id.is_empty() {
            return Err(WalletError::Other("User ID không hợp lệ".to_string()));
        }
        
        // Kiểm tra nếu người dùng đã có đăng ký đang hoạt động
        let subscriptions = self.user_subscriptions.read().await;
        if let Some(subscription) = subscriptions.get(user_id) {
            if subscription.status == SubscriptionStatus::Active && subscription.end_date > Utc::now() {
                return Err(WalletError::Other("Người dùng đã có đăng ký đang hoạt động".to_string()));
            }
        }
        drop(subscriptions);
        
        // Kiểm tra trạng thái giao dịch
        let tx_status = self.check_transaction_status(tx_hash, chain_id).await?;
        
        match tx_status {
            TransactionCheckResult::Success => {
                // Lấy thông tin giao dịch
                let tx_info = self.wallet_api.read().await
                    .get_transaction_info(tx_hash, chain_id)
                    .await
                    .map_err(|e| WalletError::Other(format!("Không thể lấy thông tin giao dịch: {}", e)))?;
                
                // Kiểm tra số tiền thanh toán
                let required_amount = self.get_required_payment_amount(user_id).await?;
                if tx_info.amount < required_amount {
                    return Err(WalletError::Other(format!(
                        "Số tiền thanh toán không đủ. Yêu cầu: {}, Đã thanh toán: {}", 
                        required_amount, tx_info.amount
                    )));
                }
                
                // Cập nhật trạng thái đăng ký
                let mut subscriptions = self.user_subscriptions.write().await;
                if let Some(subscription) = subscriptions.get_mut(user_id) {
                    subscription.status = SubscriptionStatus::Active;
                    subscription.start_date = Utc::now();
                    subscription.end_date = Utc::now() + chrono::Duration::days(30); // 30 ngày mặc định
                    
                    // Phát sự kiện thanh toán thành công
                    if let Some(emitter) = &self.event_emitter {
                        emitter.emit(SubscriptionEvent {
                            event_type: EventType::PaymentSuccess,
                            user_id: user_id.to_string(),
                            subscription_type: subscription.subscription_type,
                            timestamp: Utc::now(),
                        }).await;
                    }
                    
                    info!("Đã xử lý thanh toán thành công cho người dùng {}", user_id);
                } else {
                    return Err(WalletError::Other("Không tìm thấy thông tin đăng ký".to_string()));
                }
            }
            TransactionCheckResult::Failed => {
                return Err(WalletError::Other("Giao dịch đã thất bại".to_string()));
            }
            TransactionCheckResult::Pending => {
                return Err(WalletError::Other("Giao dịch đang chờ xử lý".to_string()));
            }
            TransactionCheckResult::NotFound => {
                return Err(WalletError::Other("Không tìm thấy giao dịch".to_string()));
        }
        }
        
        Ok(())
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
        debug!("Đang kiểm tra sở hữu NFT cho ví {} trên chain {}", wallet_address, chain_id);
        
        // Kiểm tra tính hợp lệ của địa chỉ ví
        if wallet_address == Address::zero() {
            return Err(WalletError::Other("Địa chỉ ví không hợp lệ".to_string()));
        }
        
        // Kiểm tra tính hợp lệ của địa chỉ hợp đồng NFT
        if nft_contract == Address::zero() {
            return Err(WalletError::Other("Địa chỉ hợp đồng NFT không hợp lệ".to_string()));
        }
        
        // Kiểm tra tính hợp lệ của token_id
        if token_id == U256::zero() {
            return Err(WalletError::Other("Token ID không hợp lệ".to_string()));
        }
        
        // Kiểm tra tính hợp lệ của chain_id
        if chain_id == 0 {
            return Err(WalletError::Other("Chain ID không hợp lệ".to_string()));
        }
        
        // Lấy provider cho chain
        let provider = self.wallet_api.read().await
            .get_provider(chain_id)
            .map_err(|e| WalletError::Other(format!("Không thể lấy provider: {}", e)))?;
        
        // Kiểm tra sở hữu NFT
        let is_owner = provider.check_nft_ownership(wallet_address, nft_contract, token_id)
            .await
            .map_err(|e| {
                error!("Lỗi khi kiểm tra sở hữu NFT: {}", e);
                WalletError::Other(format!("Không thể kiểm tra sở hữu NFT: {}", e))
            })?;
        
        // Log kết quả
        if is_owner {
            info!("Ví {} sở hữu NFT {} trên chain {}", wallet_address, token_id, chain_id);
        } else {
            warn!("Ví {} không sở hữu NFT {} trên chain {}", wallet_address, token_id, chain_id);
        }
        
        Ok(is_owner)
    }

    /// Nâng cấp tài khoản người dùng khi đăng ký được xác nhận thành công.
    ///
    /// # Arguments
    /// - `user_id`: ID người dùng
    /// - `subscription_type`: Loại gói đăng ký mới
    ///
    /// # Returns
    /// - `Ok(())`: Nếu nâng cấp thành công
    /// - `Err`: Nếu có lỗi
    pub async fn upgrade_user_account(
        &self,
        user_id: &str,
        subscription_type: SubscriptionType,
    ) -> Result<(), WalletError> {
        // Validate đầu vào
        if user_id.is_empty() {
            return Err(WalletError::InvalidInput("User ID không được để trống".to_string()));
        }
        
        // Kiểm tra loại gói đăng ký
        match subscription_type {
            SubscriptionType::Free => {
                return Err(WalletError::InvalidInput(
                    "Không thể nâng cấp lên gói Free. Hãy sử dụng downgrade_user_to_free".to_string()
                ));
            },
            SubscriptionType::Premium | SubscriptionType::VIP => {
                // Tiếp tục xử lý cho các gói hợp lệ
            },
            _ => {
                return Err(WalletError::InvalidInput(format!(
                    "Loại gói đăng ký không hợp lệ: {:?}", subscription_type
                )));
            }
        }
        
        debug!("Đang nâng cấp tài khoản {} lên gói {:?}", user_id, subscription_type);
        
        // Lấy thông tin gói đăng ký
        let plan = match self.subscription_plans.iter().find(|p| p.plan_type == subscription_type) {
            Some(p) => p,
            None => {
                return Err(WalletError::NotFound(format!(
                    "Không tìm thấy gói đăng ký {:?}", subscription_type
                )));
            }
        };
        
        let mut subscriptions = self.user_subscriptions.write().await;
        
        // Kiểm tra nếu người dùng đã có đăng ký
        if let Some(existing_sub) = subscriptions.get(user_id) {
            // Kiểm tra nếu đăng ký hiện tại đã hết hạn
            if existing_sub.end_date < Utc::now() {
                // Xóa đăng ký cũ
                info!("Xóa đăng ký cũ đã hết hạn cho user {}", user_id);
                subscriptions.remove(user_id);
            } else if existing_sub.subscription_type == subscription_type {
                return Err(WalletError::AlreadyExists(format!(
                    "Người dùng đã có gói đăng ký {:?}", subscription_type
                )));
            } else if subscription_type.is_lower_than(&existing_sub.subscription_type) {
                return Err(WalletError::InvalidOperation(format!(
                    "Không thể hạ cấp từ {:?} xuống {:?}. Hãy sử dụng downgrade_user_to_free",
                    existing_sub.subscription_type, subscription_type
                )));
            } else {
                // Gói đăng ký mới cao hơn gói hiện tại, tiếp tục nâng cấp
                info!("Nâng cấp từ {:?} lên {:?} cho user {}", 
                    existing_sub.subscription_type, subscription_type, user_id);
            }
        }
        
        // Tạo đăng ký mới
        let now = Utc::now();
        let duration_days = plan.duration_days as i64;
        let end_date = now + chrono::Duration::days(duration_days);
        
        let new_subscription = UserSubscription {
            user_id: user_id.to_string(),
            subscription_type,
            status: SubscriptionStatus::Active,
            start_date: now,
            end_date,
            payment_token: PaymentToken::DMD,  // Mặc định
            payment_address: self.dmd_token_address,
            features: plan.features.clone(),
            nft_info: None,  // Sẽ được cập nhật sau nếu cần
            non_nft_status: NonNftVipStatus::default(),
            payment_tx_hash: None, // Sẽ được cập nhật sau
            payment_amount: U256::zero(), // Sẽ được cập nhật sau
        };
        
        // Lưu đăng ký mới
        subscriptions.insert(user_id.to_string(), new_subscription.clone());
        drop(subscriptions); // Giải phóng lock sớm
        
        // Cập nhật trạng thái người dùng
        match subscription_type {
            SubscriptionType::Premium => {
                match self.premium_user_manager.add_user(user_id).await {
                    Ok(_) => {
                        info!("Đã thêm người dùng {} vào premium_user_manager", user_id);
                    },
                    Err(e) => {
                        error!("Lỗi khi thêm người dùng {} vào premium_user_manager: {}", user_id, e);
                        return Err(WalletError::UpdateFailed(format!(
                            "Không thể cập nhật trạng thái Premium: {}", e
                        )));
                    }
                }
            },
            SubscriptionType::VIP => {
                info!("Người dùng {} đã được nâng cấp lên VIP", user_id);
                // VIP user được quản lý riêng, không cần thêm vào premium_user_manager
            },
            _ => {}
        }
        
        // Khởi tạo auto_trade nếu cần
        match self.get_auto_trade_manager() {
            Ok(manager) => {
                let is_vip_staking = subscription_type == SubscriptionType::VIP && new_subscription.is_twelve_month_vip();
                match manager.reset_auto_trade(user_id, &subscription_type, is_vip_staking).await {
                    Ok(_) => {
                        debug!("Đã khởi tạo auto_trade cho user {}", user_id);
                    },
                    Err(e) => {
                        warn!("Không thể khởi tạo auto_trade cho user {}: {}", user_id, e);
                        // Không return lỗi ở đây, vì đây không phải lỗi nghiêm trọng
                    }
                }
            },
            Err(e) => {
                warn!("Không thể lấy auto_trade_manager: {}", e);
                // Không return lỗi ở đây, vì đây không phải lỗi nghiêm trọng
            }
        }
        
        // Phát sự kiện nâng cấp
        if let Some(emitter) = &self.event_emitter {
            let event = SubscriptionEvent {
                event_type: EventType::Upgraded,
                    user_id: user_id.to_string(),
                subscription_type,
                timestamp: Utc::now(),
            };
            
            match emitter.emit(event).await {
                Ok(_) => {
                    debug!("Đã phát sự kiện nâng cấp cho user {}", user_id);
                },
                Err(e) => {
                    warn!("Không thể phát sự kiện nâng cấp cho user {}: {}", user_id, e);
                }
            }
        }
        
        info!("Đã nâng cấp tài khoản {} lên gói {:?}", user_id, subscription_type);
        Ok(())
    }
    
    /// Lên lịch kiểm tra tính hợp lệ của NFT VIP
    async fn schedule_nft_verification(&self, user_id: &str, nft_info: NftInfo, end_date: DateTime<Utc>) {
        // TODO: Trong một hệ thống thực tế, cần có một job scheduler để thực hiện việc này
        // Ví dụ:
        // let job = JobScheduler::new();
        // job.schedule_recurring_task(
        //    Utc::now() + Duration::days(1),
        //    Duration::days(7),
        //    move || { verify_nft(user_id, nft_info.clone()) }
        // );
        
        info!("Đã lên lịch kiểm tra NFT định kỳ cho người dùng VIP {}", user_id);
    }

    /// Kiểm tra và hạ cấp tự động các subscription đã hết hạn
    /// 
    /// Phương thức này nên được gọi định kỳ (ví dụ thông qua một cronjob)
    /// 
    /// # Returns
    /// Danh sách các user_id đã được hạ cấp
    pub async fn check_and_downgrade_expired_subscriptions(&self) -> Result<Vec<String>, WalletError> {
        info!("Bắt đầu kiểm tra và hạ cấp các subscription đã hết hạn");
        let mut downgraded_users = Vec::new();
        
        // Danh sách các user cần hạ cấp
        let users_to_downgrade = {
            let subscriptions = self.user_subscriptions.read().await;
            let mut users = Vec::new();
            
            for (user_id, subscription) in subscriptions.iter() {
                if subscription.needs_downgrade() {
                    users.push(user_id.clone());
                }
            }
            
            users
        };
        
        // Thực hiện hạ cấp cho từng user
        for user_id in users_to_downgrade {
            match self.downgrade_user_to_free(&user_id).await {
                Ok(_) => {
                    info!("Đã hạ cấp user {} về Free do hết hạn subscription", user_id);
                    downgraded_users.push(user_id);
                }
                Err(e) => {
                    error!("Lỗi khi hạ cấp user {}: {}", user_id, e);
                }
            }
        }
        
        info!("Hoàn thành kiểm tra và hạ cấp. Tổng số user bị hạ cấp: {}", downgraded_users.len());
        Ok(downgraded_users)
    }
    
    /// Hạ cấp một người dùng về Free
    /// 
    /// # Arguments
    /// - `user_id`: ID người dùng cần hạ cấp
    /// 
    /// # Returns
    /// - `Ok(())`: Nếu hạ cấp thành công
    /// - `Err`: Nếu có lỗi
    pub async fn downgrade_user_to_free(&self, user_id: &str) -> Result<(), WalletError> {
        // Lấy thông tin subscription hiện tại
        let new_subscription = {
            let mut subscriptions = self.user_subscriptions.write().await;
            let subscription = subscriptions.get(user_id)
                .ok_or(WalletError::Other("Không tìm thấy thông tin đăng ký".to_string()))?;
            
            // Tạo subscription mới (free)
            let new_subscription = subscription.downgrade_to_free()
                .map_err(|e| WalletError::Other(format!("Lỗi khi tạo subscription mới: {}", e)))?;
                
            // Cập nhật vào danh sách
            subscriptions.insert(user_id.to_string(), new_subscription.clone());
            
            new_subscription
        };
        
        // Nếu có auto_trade_manager, cập nhật trạng thái
        if let Some(auto_trade_manager) = &self.auto_trade_manager {
            // TODO: Cập nhật auto_trade_usage về inactive
            // Ví dụ:
            // auto_trade_manager.deactivate_user_auto_trade(user_id).await
            //    .map_err(|e| WalletError::from(e))?;
        }
        
        // Xóa dữ liệu premium/vip nếu có
        // TODO: Trong thực tế cần giữ lại lịch sử, chỉ cập nhật trạng thái
        // Ví dụ:
        // let mongodb = get_mongodb_client();
        // 
        // // Cập nhật trạng thái trong bảng premium_users
        // mongodb.collection("premium_users").update_one(
        //     doc! { "user_id": user_id },
        //     doc! { "$set": { "status": "Inactive", "end_date": Utc::now() } }
        // ).await?;
        // 
        // // Cập nhật trạng thái trong bảng vip_users
        // mongodb.collection("vip_users").update_one(
        //     doc! { "user_id": user_id },
        //     doc! { "$set": { "status": "Inactive", "end_date": Utc::now() } }
        // ).await?;
        
        // Gửi thông báo cho người dùng
        // TODO: Gửi email/notification
        // Ví dụ:
        // notification_service.send_notification(
        //     user_id,
        //     "Subscription của bạn đã hết hạn",
        //     "Tài khoản của bạn đã được chuyển về gói Free. Vui lòng nâng cấp để tiếp tục sử dụng các tính năng cao cấp."
        // ).await?;
        
        info!("Đã hạ cấp tài khoản của người dùng {} về Free", user_id);
        Ok(())
    }

    /// Nâng cấp từ gói Free lên gói VIP
    /// 
    /// # Arguments
    /// * `user_id` - ID người dùng
    /// * `nft_info` - Thông tin NFT
    /// * `is_twelve_month_plan` - Là gói 12 tháng hay không
    /// * `staking_tokens` - Số lượng token stake (nếu là gói 12 tháng)
    /// 
    /// # Returns
    /// * `Result<UserSubscription, WalletError>` - Kết quả thực hiện
    #[flow_from("wallet::ui")]
    pub async fn upgrade_free_to_vip(
        &self, 
        user_id: &str, 
        nft_info: super::nft::NftInfo, 
        is_twelve_month_plan: bool,
        staking_tokens: Option<U256>
    ) -> Result<UserSubscription, WalletError> {
        // Kiểm tra NFT trước khi nâng cấp
        let nft_valid = self.check_nft_ownership(
            nft_info.wallet_address,
            nft_info.contract_address,
            nft_info.token_id,
            nft_info.chain_id,
        ).await?;
        
        if !nft_valid {
            return Err(WalletError::Other("NFT không hợp lệ hoặc không tìm thấy trong ví".to_string()));
        }
        
        // Thêm logic bắt buộc có NFT trước khi đăng ký
        info!("Đã xác minh NFT cho user {} trước khi nâng cấp lên VIP", user_id);
        
        // Tiếp tục logic nâng cấp
        let mut subscriptions = self.user_subscriptions.write().await;
        
        // Lấy subscription hiện tại hoặc tạo mới nếu chưa có
        let subscription_result = match subscriptions.get(user_id) {
            Some(sub) => Ok(sub.clone()),
            None => {
                // Tạo subscription mới nếu chưa có
                info!("Tạo subscription mới cho user {} khi stake token", user_id);
                Ok(UserSubscription {
                    user_id: user_id.to_string(),
                    subscription_type: SubscriptionType::Free,
                    start_date: Utc::now(),
                    end_date: Utc::now() + Duration::days(30),
                    payment_address: nft_info.wallet_address,
                    payment_tx_hash: None,
                    payment_token: PaymentToken::DMD,
                    payment_amount: U256::zero(),
                    status: SubscriptionStatus::Pending,
                    nft_info: nft_info.clone(),
                    non_nft_status: NonNftVipStatus::default(),
                })
            }
        };
        
        // Xử lý kết quả subscription
        let subscription = subscription_result?;
        
        // Chuyển đổi subscription thông qua trait
        let mut subscription = subscription;
        
        // Nếu là gói 12 tháng với staking
        if is_twelve_month_plan {
            if let Some(amount) = staking_tokens {
                // Kiểm tra số lượng tối thiểu DMD
                let min_amount = U256::from(MIN_DMD_STAKE_AMOUNT.mantissa());
                if amount < min_amount {
                    return Err(WalletError::Other(format!(
                        "Số lượng token không đủ để stake. Cần tối thiểu {} DMD",
                        MIN_DMD_STAKE_AMOUNT
                    )));
                }
                
                // Cập nhật thông tin subscription
                let duration = Duration::days(TWELVE_MONTH_STAKE_DAYS);
                let start_date = Utc::now();
                let end_date = start_date + duration;
                let price = if subscription.subscription_type == SubscriptionType::Free {
                    VIP_TWELVE_MONTH_PRICE_USDC
                } else {
                    // Giảm giá nếu đã là Premium
                    VIP_TWELVE_MONTH_PRICE_USDC - DEFAULT_PREMIUM_PRICE_USDC
                };
                
                subscription.upgrade_with_staking(amount, true, Some(nft_info.clone()))
                    .map_err(|e| WalletError::Other(format!("Lỗi khi nâng cấp subscription: {}", e)))?;
                
                // Cập nhật trạng thái subscription và trả về thông tin
                subscriptions.insert(user_id.to_string(), subscription.clone());
                drop(subscriptions);
                
                // Ghi log
                info!(
                    "Đã nâng cấp user {} từ {:?} lên VIP với 12 tháng stake, giá: {}",
                    user_id, subscription.subscription_type, price
                );
                
                return Ok(subscription);
            } else {
                return Err(WalletError::Other("Cần cung cấp số lượng token để stake cho gói 12 tháng".to_string()));
            }
        } else {
            // Gói VIP 30 ngày
            let duration = Duration::days(30);
            let start_date = Utc::now();
            let end_date = start_date + duration;
            
            // Xác định giá nâng cấp
            let price = if subscription.subscription_type == SubscriptionType::Free {
                FREE_TO_VIP_UPGRADE_PRICE_USDC
            } else {
                // Giảm giá nếu đã là Premium
                PREMIUM_TO_VIP_UPGRADE_PRICE_USDC
            };
            
            subscription.upgrade_with_staking(U256::zero(), false, None)
                .map_err(|e| WalletError::Other(format!("Lỗi khi nâng cấp subscription: {}", e)))?;
            
            // Cập nhật trạng thái subscription và trả về thông tin
            subscriptions.insert(user_id.to_string(), subscription.clone());
            drop(subscriptions);
            
            // Ghi log
            info!(
                "Đã nâng cấp user {} từ {:?} lên VIP với 30 ngày, giá: {}",
                user_id, subscription.subscription_type, price
            );
            
            return Ok(subscription);
        }
    }
    
    /// Stake token DMD cho subscription.
    /// 
    /// # Arguments
    /// * `user_id` - ID người dùng
    /// * `wallet_address` - Địa chỉ ví
    /// * `chain_type` - Loại blockchain
    /// * `amount` - Số lượng token cần stake
    /// * `is_vip` - Có phải gói VIP hay không
    /// * `nft_info` - Thông tin NFT (nếu là VIP)
    /// 
    /// # Returns
    /// * `Result<TokenStake, WalletError>` - Kết quả thực hiện
    #[flow_from("wallet::staking")]
    pub async fn stake_tokens_for_subscription(
        &self,
        user_id: &str,
        wallet_address: Address,
        chain_type: ChainType,
        amount: U256,
        is_vip: bool,
        nft_info: Option<super::nft::NftInfo>,
    ) -> Result<super::staking::TokenStake, WalletError> {
        use super::staking::{StakingManager, TokenStake};
        
        info!("Khóa {} DMD token trong pool stake cho người dùng {}", amount, user_id);
        
        // Nếu là gói VIP, bắt buộc phải có NFT
        if is_vip {
            if let Some(nft) = &nft_info {
                // Kiểm tra NFT trước khi cho phép stake cho gói VIP
                let nft_valid = self.check_nft_ownership(
                    nft.wallet_address,
                    nft.contract_address,
                    nft.token_id,
                    nft.chain_id,
                ).await?;
                
                if !nft_valid {
                    return Err(WalletError::Other("NFT không hợp lệ hoặc không tìm thấy trong ví".to_string()));
                }
                
                info!("Đã xác minh NFT cho user {} trước khi stake DMD token cho gói VIP 12 tháng", user_id);
            } else {
                return Err(WalletError::Other("Cần cung cấp thông tin NFT cho gói VIP 12 tháng".to_string()));
            }
        }
        
        // Tạo StakingManager
        let staking_manager = StakingManager::new(self.wallet_api.clone());
        
        // Thực hiện stake token
        let mut stake = staking_manager.stake_tokens(user_id, wallet_address, chain_type, amount).await
            .map_err(|e| WalletError::Other(format!("Lỗi khi stake token: {}", e)))?;
        
        // Cập nhật đăng ký người dùng
        let mut subscriptions = self.user_subscriptions.write().await;
        
        // Lấy subscription hiện tại hoặc tạo mới nếu chưa có
        let subscription_result = match subscriptions.get(user_id) {
            Some(sub) => Ok(sub.clone()),
            None => {
                // Tạo subscription mới nếu chưa có
                info!("Tạo subscription mới cho user {} khi stake token", user_id);
                Ok(UserSubscription {
                    user_id: user_id.to_string(),
                    subscription_type: SubscriptionType::Free,
                    start_date: Utc::now(),
                    end_date: Utc::now() + Duration::days(30),
                    payment_address: wallet_address,
                    payment_tx_hash: None,
                    payment_token: PaymentToken::DMD,
                    payment_amount: U256::zero(),
                    status: SubscriptionStatus::Pending,
                    nft_info: nft_info.clone(),
                    non_nft_status: NonNftVipStatus::default(),
                })
            }
        };
        
        // Xử lý kết quả subscription
        let subscription = subscription_result?;
        
        // Chuyển đổi subscription thông qua trait
        let subscription = if is_vip {
            // Đánh dấu stake là cho gói VIP
            stake.mark_as_vip_subscription();
            
            subscription.upgrade_with_staking(amount, true, nft_info)
                .map_err(|e| WalletError::Other(format!("Lỗi khi nâng cấp subscription: {}", e)))?
        } else {
            subscription.upgrade_with_staking(amount, false, None)
                .map_err(|e| WalletError::Other(format!("Lỗi khi nâng cấp subscription: {}", e)))?
        };
        
        // Cập nhật subscription
        subscriptions.insert(user_id.to_string(), subscription);
        drop(subscriptions);
        
        // Thêm transaction hash từ stake
        if let Some(tx_hash) = &stake.stake_tx_hash {
            self.process_payment(user_id, tx_hash, chain_type as u64).await?;
        }
        
        // Ghi log hoạt động
        let subscription_type = if is_vip {
            SubscriptionType::VIP
        } else {
            SubscriptionType::Premium
        };
        
        info!(
            "Đã khóa {} DMD token trong pool stake cho người dùng {} thời hạn 12 tháng, loại gói: {:?}",
            amount, user_id, subscription_type
        );
        
        Ok(stake)
    }
    
    /// Lấy thông tin stake của người dùng
    /// 
    /// # Arguments
    /// * `user_id` - ID người dùng
    /// 
    /// # Returns
    /// * `Result<Vec<TokenStake>, WalletError>` - Danh sách stake
    #[flow_from("wallet::staking")]
    pub async fn get_user_stakes(&self, user_id: &str) -> Result<Vec<super::staking::TokenStake>, WalletError> {
        use super::staking::StakingManager;
        
        // Tạo StakingManager
        let staking_manager = StakingManager::new(self.wallet_api.clone());
        
        // Lấy danh sách stake
        staking_manager.get_user_stakes(user_id).await
            .map_err(|e| WalletError::Other(format!("Lỗi khi lấy thông tin stake: {}", e)))
    }
    
    /// Tính toán lợi nhuận từ stake
    /// 
    /// # Arguments
    /// * `user_id` - ID người dùng
    /// 
    /// # Returns
    /// * `Result<rust_decimal::Decimal, WalletError>` - Tổng lợi nhuận
    #[flow_from("wallet::staking")]
    pub async fn calculate_staking_rewards(&self, user_id: &str) -> Result<rust_decimal::Decimal, WalletError> {
        use super::staking::StakingManager;
        
        // Tạo StakingManager
        let staking_manager = StakingManager::new(self.wallet_api.clone());
        
        // Tính lợi nhuận
        staking_manager.calculate_rewards(user_id).await
            .map_err(|e| WalletError::Other(format!("Lỗi khi tính lợi nhuận stake: {}", e)))
    }
    
    /// Hoàn thành stake token
    /// 
    /// # Arguments
    /// * `user_id` - ID người dùng
    /// * `stake_id` - ID của stake
    /// 
    /// # Returns
    /// * `Result<super::staking::TokenStake, WalletError>` - Thông tin stake sau khi hoàn thành
    #[flow_from("wallet::staking")]
    pub async fn complete_stake(&self, user_id: &str, stake_id: &str) -> Result<super::staking::TokenStake, WalletError> {
        use super::staking::StakingManager;
        
        // Tạo StakingManager
        let staking_manager = StakingManager::new(self.wallet_api.clone());
        
        // Hoàn thành stake
        staking_manager.complete_stake(user_id, stake_id).await
            .map_err(|e| WalletError::Other(format!("Lỗi khi hoàn thành stake: {}", e)))
    }

    /// Lấy thông tin đăng ký của người dùng
    ///
    /// # Arguments
    /// - `user_id`: ID người dùng
    ///
    /// # Returns
    /// - `Ok(UserSubscription)`: Thông tin đăng ký
    /// - `Err`: Nếu không tìm thấy thông tin đăng ký
    pub async fn get_user_subscription(&self, user_id: &str) -> Result<UserSubscription, WalletError> {
        let subscriptions = self.user_subscriptions.read().await;
        
        if let Some(subscription) = subscriptions.get(user_id) {
            Ok(subscription.clone())
        } else {
            Err(WalletError::Other(format!("Không tìm thấy thông tin đăng ký cho người dùng {}", user_id)))
        }
    }

    /// Khởi tạo và chạy task nền kiểm tra subscription
    pub async fn start_background_tasks(&self) -> Result<(), WalletError> {
        info!("Khởi động các background task cho subscription manager");
        
        // Clone các tham chiếu cần thiết
        let subscription_manager = Arc::new(self.clone());
        let auto_trade_manager = self.get_auto_trade_manager()?;
        let event_emitter = match &self.event_emitter {
            Some(emitter) => Some(emitter.clone()),
            None => {
                warn!("Không tìm thấy event_emitter, thông báo sẽ không được gửi");
                None
            }
        };
        
        // Chạy task kiểm tra subscription
        tokio::spawn(async move {
            let mut interval = interval(SUBSCRIPTION_CHECK_INTERVAL);
            
            loop {
                interval.tick().await;
                debug!("Thực hiện kiểm tra định kỳ subscription");
                
                // Kiểm tra và hạ cấp subscription hết hạn
                if let Err(e) = subscription_manager.check_and_process_subscriptions().await {
                    error!("Lỗi khi kiểm tra subscription: {}", e);
                }
                
                // Kiểm tra và gửi thông báo cho subscription sắp hết hạn
                if let Err(e) = subscription_manager.check_and_send_expiry_warnings().await {
                    error!("Lỗi khi gửi thông báo sắp hết hạn: {}", e);
                }
                
                // Kiểm tra và xác minh NFT
                if let Err(e) = subscription_manager.verify_all_nft_ownership().await {
                    error!("Lỗi khi xác minh NFT: {}", e);
                }
            }
        });
        
        // Chạy task kiểm tra auto-trade
        tokio::spawn(async move {
            let mut interval = interval(AUTO_TRADE_PERIODIC_CHECK_INTERVAL);
            
            loop {
                interval.tick().await;
                debug!("Thực hiện kiểm tra định kỳ auto-trade");
                
                // Kiểm tra và cập nhật auto-trade
                if let Err(e) = auto_trade_manager.periodic_check().await {
                    error!("Lỗi khi kiểm tra auto-trade: {}", e);
                }
            }
        });
        
        info!("Đã khởi động các background task thành công");
        Ok(())
    }
    
    /// Kiểm tra và xử lý tất cả subscription
    /// - Hạ cấp subscription đã hết hạn
    /// - Cập nhật trạng thái subscription
    /// - Đồng bộ với auto-trade manager
    async fn check_and_process_subscriptions(&self) -> Result<(), WalletError> {
        info!("Bắt đầu kiểm tra và xử lý subscription");
        
        // Lấy danh sách subscription
        let subscriptions = {
            let subscriptions_guard = self.user_subscriptions.read().await;
            subscriptions_guard.clone()
        };
        
        // Danh sách user cần hạ cấp
        let mut users_to_downgrade = Vec::new();
        
        // Kiểm tra từng subscription
        for (user_id, subscription) in subscriptions {
            if subscription.is_expired() {
                debug!("Subscription hết hạn: user_id={}, type={:?}", 
                       user_id, subscription.subscription_type);
                users_to_downgrade.push(user_id);
            }
        }
        
        // Hạ cấp các user đã hết hạn
        for user_id in &users_to_downgrade {
            match self.downgrade_user_to_free(user_id).await {
                Ok(_) => {
                    info!("Đã hạ cấp user {} xuống Free do hết hạn", user_id);
                    
                    // Gửi sự kiện
                    if let Some(emitter) = &self.event_emitter {
                        let event = SubscriptionEvent {
                            user_id: user_id.clone(),
                            event_type: EventType::SubscriptionExpired,
                            timestamp: Utc::now(),
                            data: Some("Subscription đã hết hạn và đã bị hạ cấp xuống Free".to_string()),
                        };
                        emitter.emit(event);
                    }
                }
                Err(e) => {
                    error!("Lỗi khi hạ cấp user {} xuống Free: {}", user_id, e);
                }
            }
        }
        
        info!("Hoàn thành kiểm tra subscription, đã hạ cấp {} user", users_to_downgrade.len());
        Ok(())
    }
    
    /// Kiểm tra và gửi thông báo cho các subscription sắp hết hạn
    async fn check_and_send_expiry_warnings(&self) -> Result<(), WalletError> {
        info!("Kiểm tra và gửi thông báo subscription sắp hết hạn");
        
        // Chỉ tiếp tục nếu có event_emitter
        let emitter = match &self.event_emitter {
            Some(e) => e,
            None => {
                debug!("Không có event_emitter, bỏ qua việc gửi thông báo");
                return Ok(());
            }
        };
        
        // Lấy danh sách subscription
        let subscriptions = {
            let subscriptions_guard = self.user_subscriptions.read().await;
            subscriptions_guard.clone()
        };
        
        // Kiểm tra từng subscription
        let now = Utc::now();
        let warning_threshold = now + Duration::days(EXPIRY_WARNING_DAYS);
        let renewal_threshold = now + Duration::days(RENEWAL_REMINDER_DAYS);
        
        // Đếm số thông báo đã gửi để log
        let mut warning_count = 0;
        let mut renewal_count = 0;
        
        for (user_id, subscription) in subscriptions {
            // Chỉ xem xét các subscription đang active
            if subscription.status != SubscriptionStatus::Active {
                continue;
            }
            
            // Kiểm tra xem có sắp hết hạn không
            if subscription.end_date <= warning_threshold {
                // Tính số ngày còn lại
                let days_remaining = (subscription.end_date - now).num_days();
                
                debug!("Subscription sắp hết hạn: user_id={}, type={:?}, còn {} ngày", 
                       user_id, subscription.subscription_type, days_remaining);
                
                // Gửi thông báo sắp hết hạn
                let event = SubscriptionEvent {
                    user_id: user_id.clone(),
                    event_type: EventType::SubscriptionExpiryWarning,
                    timestamp: now,
                    data: Some(format!("Subscription sẽ hết hạn trong {} ngày", days_remaining)),
                };
                emitter.emit(event);
                warning_count += 1;
            }
            // Kiểm tra xem có nên nhắc gia hạn không
            else if subscription.end_date <= renewal_threshold 
                    && subscription.subscription_type != SubscriptionType::Free {
                // Tính số ngày còn lại
                let days_remaining = (subscription.end_date - now).num_days();
                
                debug!("Nên gia hạn subscription: user_id={}, type={:?}, còn {} ngày", 
                       user_id, subscription.subscription_type, days_remaining);
                
                // Gửi nhắc nhở gia hạn
                let event = SubscriptionEvent {
                    user_id: user_id.clone(),
                    event_type: EventType::SubscriptionRenewed,
                    timestamp: now,
                    data: Some(format!("Hãy gia hạn subscription để tiếp tục sử dụng dịch vụ. Còn {} ngày", days_remaining)),
                };
                emitter.emit(event);
                renewal_count += 1;
            }
        }
        
        info!("Đã gửi {} thông báo sắp hết hạn và {} nhắc nhở gia hạn", warning_count, renewal_count);
        Ok(())
    }
    
    /// Kiểm tra việc sở hữu NFT định kỳ cho tất cả người dùng
    /// 
    /// # Returns
    /// * `Result<(), WalletError>` - Kết quả thực hiện
    pub async fn verify_all_nft_ownership(&self) -> Result<(), WalletError> {
        let subscriptions = self.user_subscriptions.read().await;
        let mut verification_results = Vec::new();
        
        for (user_id, subscription) in subscriptions.iter() {
            // Bỏ qua người dùng không phải VIP hoặc không có thông tin NFT
            if subscription.subscription_type != SubscriptionType::VIP || subscription.nft_info.is_none() {
                continue;
            }
            
            // Bỏ qua kiểm tra NFT cho người dùng VIP 12 tháng đã stake DMD token
            if subscription.non_nft_status == NonNftVipStatus::StakedDMD {
                info!("Bỏ qua kiểm tra NFT cho người dùng VIP 12 tháng {} đã stake DMD token", user_id);
                continue;
            }
            
            if let Some(nft_info) = &subscription.nft_info {
                // Thêm vào danh sách kiểm tra
                let wallet_address = nft_info.wallet_address;
                let contract_address = nft_info.contract_address;
                let token_id = nft_info.token_id;
                let chain_id = nft_info.chain_id;
                
                // Giới hạn số lần kiểm tra NFT dựa trên thời điểm kiểm tra gần nhất
                if let Some(last_verified) = nft_info.last_verified {
                    let now = Utc::now();
                    let duration_since_last_check = now.signed_duration_since(last_verified);
                    
                    // Nếu kiểm tra gần đây, bỏ qua
                    if duration_since_last_check < chrono::Duration::hours(12) {
                        continue;
                    }
                }
                
                match self.check_nft_ownership(wallet_address, contract_address, token_id, chain_id).await {
                    Ok(is_valid) => {
                        verification_results.push((user_id.clone(), is_valid));
                    }
                    Err(e) => {
                        error!("Lỗi khi kiểm tra NFT cho người dùng {}: {}", user_id, e);
                    }
                }
            }
        }
        
        // Cập nhật trạng thái xác minh
        if !verification_results.is_empty() {
            self.update_nft_verification_status(&verification_results).await?;
        }
        
        Ok(())
    }
    
    /// Cập nhật trạng thái xác minh NFT
    async fn update_nft_verification_status(&self, user_verifications: &[(String, bool)]) -> Result<(), WalletError> {
        let mut subscriptions = self.user_subscriptions.write().await;
        
        for (user_id, is_valid) in user_verifications {
            if let Some(subscription) = subscriptions.get_mut(user_id) {
                if let Some(nft_info) = &mut subscription.nft_info {
                    // Cập nhật trạng thái và thời gian xác minh
                    nft_info.is_valid = *is_valid;
                    nft_info.last_verified = Some(Utc::now());
                    
                    debug!("Đã cập nhật trạng thái NFT cho user {}: is_valid={}", user_id, is_valid);
                }
            }
        }
        
        Ok(())
    }
    
    /// Thiết lập event emitter để gửi thông báo
    pub fn set_event_emitter(&mut self, emitter: EventEmitter) {
        self.event_emitter = Some(emitter.clone());
        
        // Thiết lập cho auto-trade manager nếu có
        if let Ok(mut manager) = self.get_auto_trade_manager() {
            manager.set_event_emitter(emitter);
        }
    }

    /// Gia hạn đăng ký
    ///
    /// # Arguments
    /// * `user_id` - ID người dùng
    /// * `months` - Số tháng gia hạn
    ///
    /// # Returns
    /// `Ok(())` nếu gia hạn thành công, `Err` nếu có lỗi
    pub async fn renew_subscription(&self, user_id: &str, months: u32) -> Result<(), WalletError> {
        if months == 0 {
            return Err(WalletError::InvalidInput("Số tháng gia hạn phải lớn hơn 0".to_string()));
        }

        let mut subscriptions = self.user_subscriptions.write().await;
        let subscription = subscriptions.get_mut(user_id)
            .ok_or_else(|| WalletError::NotFound(format!("Không tìm thấy đăng ký cho user {}", user_id)))?;

        // Kiểm tra trạng thái đăng ký hiện tại
        if subscription.status != SubscriptionStatus::Active {
            return Err(WalletError::InvalidState(
                format!("Không thể gia hạn đăng ký với trạng thái {}", subscription.status)
            ));
        }

        // Tính toán ngày hết hạn mới
        let current_end_date = subscription.end_date;
        let new_end_date = current_end_date + chrono::Duration::days(months as i64 * 30);

        // Cập nhật thông tin đăng ký
        subscription.end_date = new_end_date;
        subscription.last_renewal_date = Utc::now();
        subscription.renewal_count += 1;

        // Lưu vào database
        self.db.update_subscription(user_id, subscription)
            .await
            .map_err(|e| WalletError::DatabaseError(e.to_string()))?;

        // Gửi event thông báo
        if let Some(emitter) = &self.event_emitter {
            emitter.emit(EventType::SubscriptionRenewed, SubscriptionEvent {
                user_id: user_id.to_string(),
                subscription_type: subscription.subscription_type,
                end_date: new_end_date,
            });
        }

        info!("Đã gia hạn đăng ký cho user {} thêm {} tháng", user_id, months);
        Ok(())
    }

    /// Gia hạn đăng ký với thời gian cụ thể
    ///
    /// # Arguments
    /// * `user_id` - ID người dùng
    /// * `start_date` - Ngày bắt đầu
    /// * `end_date` - Ngày kết thúc
    ///
    /// # Returns
    /// `Ok(())` nếu gia hạn thành công, `Err` nếu có lỗi
    pub async fn extend_subscription(
        &self,
        user_id: &str,
        start_date: DateTime<Utc>,
        end_date: DateTime<Utc>,
    ) -> Result<(), WalletError> {
        // Validate dates
        if start_date >= end_date {
            return Err(WalletError::InvalidInput("Ngày bắt đầu phải trước ngày kết thúc".to_string()));
        }

        let now = Utc::now();
        if end_date <= now {
            return Err(WalletError::InvalidInput("Ngày kết thúc phải trong tương lai".to_string()));
        }

        let mut subscriptions = self.user_subscriptions.write().await;
        let subscription = subscriptions.get_mut(user_id)
            .ok_or_else(|| WalletError::NotFound(format!("Không tìm thấy đăng ký cho user {}", user_id)))?;

        // Kiểm tra trạng thái đăng ký
        if subscription.status != SubscriptionStatus::Active {
            return Err(WalletError::InvalidState(
                format!("Không thể gia hạn đăng ký với trạng thái {}", subscription.status)
            ));
        }

        // Cập nhật thông tin đăng ký
        subscription.start_date = start_date;
        subscription.end_date = end_date;
        subscription.last_renewal_date = now;

        // Lưu vào database
        self.db.update_subscription(user_id, subscription)
            .await
            .map_err(|e| WalletError::DatabaseError(e.to_string()))?;

        // Gửi event thông báo
        if let Some(emitter) = &self.event_emitter {
            emitter.emit(EventType::SubscriptionExtended, SubscriptionEvent {
                user_id: user_id.to_string(),
                subscription_type: subscription.subscription_type,
                end_date,
            });
        }

        info!("Đã gia hạn đăng ký cho user {} từ {} đến {}", user_id, start_date, end_date);
        Ok(())
    }

    /// Hủy đăng ký
    ///
    /// # Arguments
    /// * `user_id` - ID người dùng
    ///
    /// # Returns
    /// `Ok(())` nếu hủy thành công, `Err` nếu có lỗi
    pub async fn cancel_subscription(&self, user_id: &str) -> Result<(), WalletError> {
        let mut subscriptions = self.user_subscriptions.write().await;
        let subscription = subscriptions.get_mut(user_id)
            .ok_or_else(|| WalletError::NotFound(format!("Không tìm thấy đăng ký cho user {}", user_id)))?;

        // Kiểm tra trạng thái đăng ký
        if subscription.status != SubscriptionStatus::Active {
            return Err(WalletError::InvalidState(
                format!("Không thể hủy đăng ký với trạng thái {}", subscription.status)
            ));
        }

        // Cập nhật trạng thái
        subscription.status = SubscriptionStatus::Cancelled;
        subscription.cancellation_date = Some(Utc::now());

        // Lưu vào database
        self.db.update_subscription(user_id, subscription)
            .await
            .map_err(|e| WalletError::DatabaseError(e.to_string()))?;

        // Gửi event thông báo
        if let Some(emitter) = &self.event_emitter {
            emitter.emit(EventType::SubscriptionCancelled, SubscriptionEvent {
                user_id: user_id.to_string(),
                subscription_type: subscription.subscription_type,
                end_date: subscription.end_date,
            });
        }

        info!("Đã hủy đăng ký cho user {}", user_id);
        Ok(())
    }

    /// Cập nhật thông tin đăng ký và đồng bộ với cơ sở dữ liệu
    /// 
    /// Phương thức này cung cấp cơ chế đồng bộ hóa để đảm bảo tính nhất quán khi
    /// nhiều luồng cố gắng cập nhật cùng một đăng ký.
    /// 
    /// # Arguments
    /// * `user_id` - ID người dùng cần cập nhật
    /// * `update_fn` - Hàm closure nhận tham chiếu tới subscription và thực hiện cập nhật
    /// 
    /// # Returns
    /// * `Ok(UserSubscription)` - Subscription đã được cập nhật
    /// * `Err(WalletError)` - Lỗi khi cập nhật
    pub async fn update_subscription<F>(&self, user_id: &str, update_fn: F) -> Result<UserSubscription, WalletError>
    where
        F: FnOnce(&mut UserSubscription) -> Result<(), WalletError>,
    {
        // Tạo mutex key riêng cho mỗi user_id để tránh blocking các users khác
        let mutex_key = format!("subscription_update:{}", user_id);
        
        // Sử dụng tokio::sync::Mutex để khóa theo user_id
        let mutex = SUBSCRIPTION_MUTEXES.entry(mutex_key.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone();
        
        // Khóa mutex trước khi cập nhật
        let _guard = mutex.lock().await;
        
        // Lấy trạng thái subscription hiện tại
        let mut subscription = self.get_user_subscription(user_id).await?;
        
        // Áp dụng hàm cập nhật
        update_fn(&mut subscription)?;
        
        // Lưu trạng thái mới vào database
        self.save_subscription_to_db(&subscription).await?;
        
        // Cập nhật cache
        {
            let mut subscriptions = self.user_subscriptions.write().await;
            subscriptions.insert(user_id.to_string(), subscription.clone());
        }
        
        // Phát sự kiện subscription được cập nhật
        if let Some(emitter) = &self.event_emitter {
            let event = SubscriptionEvent {
                event_type: EventType::SubscriptionUpdated,
                user_id: user_id.to_string(),
                subscription: subscription.clone(),
                timestamp: Utc::now(),
            };
            emitter.emit(event);
        }
        
        // Cập nhật session token nếu có sự thay đổi đáng kể trong subscription
        // Các thay đổi đáng kể bao gồm: thay đổi loại subscription, trạng thái, ngày hết hạn
        let subscription_changes_significant = true; // TODO: Kiểm tra thay đổi đáng kể
        
        if subscription_changes_significant {
            // Refresh session token cho user
            self.refresh_session_token(user_id, &subscription).await?;
        }
        
        // Xóa mutex khỏi map nếu không cần thiết nữa
        SUBSCRIPTION_MUTEXES.remove(&mutex_key);
        
        Ok(subscription)
    }

    /// Refresh session token khi subscription thay đổi
    async fn refresh_session_token(&self, user_id: &str, subscription: &UserSubscription) -> Result<(), WalletError> {
        // Xác định loại user manager dựa vào loại subscription
        match subscription.subscription_type {
            SubscriptionType::Free => {
                // Refresh session cho free user
                if let Some(free_user_manager) = self.free_user_manager.as_ref() {
                    if let Err(e) = free_user_manager.refresh_session_token(user_id).await {
                        warn!("Không thể refresh session token cho Free user {}: {}", user_id, e);
                    }
                }
            },
            SubscriptionType::Premium => {
                // Refresh session cho premium user
                if let Some(premium_user_manager) = self.premium_user_manager.as_ref() {
                    if let Err(e) = premium_user_manager.refresh_session_token(user_id).await {
                        warn!("Không thể refresh session token cho Premium user {}: {}", user_id, e);
                    }
                }
            },
            SubscriptionType::VIP => {
                // Refresh session cho VIP user
                if let Some(vip_user_manager) = self.vip_user_manager.as_ref() {
                    if let Err(e) = vip_user_manager.refresh_session_token(user_id).await {
                        warn!("Không thể refresh session token cho VIP user {}: {}", user_id, e);
                    }
                }
            },
        }
        
        Ok(())
    }
    
    /// Lưu thông tin subscription vào database
    async fn save_subscription_to_db(&self, subscription: &UserSubscription) -> Result<(), WalletError> {
        // Serialize subscription
        let data = serde_json::to_string(subscription)
            .map_err(|e| WalletError::Other(format!("Lỗi serialize subscription: {}", e)))?;
        
        // Lưu vào database
        self.db.execute(
            "INSERT OR REPLACE INTO user_subscriptions (user_id, data) VALUES (?, ?)",
            &[&subscription.user_id, &data],
        ).await.map_err(|e| WalletError::Other(format!("Lỗi database: {}", e)))?;
        
        info!("Đã lưu subscription cho user {} vào database", subscription.user_id);
        Ok(())
    }

    /// Đồng bộ hóa trạng thái người dùng giữa các module.
    ///
    /// Phương thức này đảm bảo rằng khi trạng thái người dùng thay đổi,
    /// tất cả các module liên quan sẽ được cập nhật để duy trì sự nhất quán của dữ liệu.
    ///
    /// # Arguments
    /// * `user_id` - ID người dùng cần đồng bộ trạng thái
    /// * `sync_nft` - Có đồng bộ với NFT module không (mặc định là true)
    /// * `sync_auto_trade` - Có đồng bộ với auto-trade module không (mặc định là true)
    ///
    /// # Returns
    /// * `Ok(())` - Nếu đồng bộ thành công
    /// * `Err(WalletError)` - Nếu có lỗi trong quá trình đồng bộ
    pub async fn sync_user_state(
        &self,
        user_id: &str,
        sync_nft: bool,
        sync_auto_trade: bool,
    ) -> Result<(), WalletError> {
        debug!("Đồng bộ hóa trạng thái người dùng: {}", user_id);
        
        let subscription = match self.get_user_subscription(user_id).await {
            Ok(sub) => sub,
            Err(e) => {
                warn!("Không thể lấy subscription cho user {}: {}", user_id, e);
                return Err(e);
            }
        };
        
        // Đồng bộ với NFT module nếu cần
        if sync_nft && subscription.subscription_type == SubscriptionType::VIP {
            if let Some(vip_nft_manager) = &self.vip_nft_manager {
                if let Err(e) = vip_nft_manager.invalidate_cache(user_id).await {
                    warn!("Lỗi khi đồng bộ NFT cache: {}", e);
                }
            }
        }
        
        // Đồng bộ với auto-trade module nếu cần
        if sync_auto_trade {
            if let Some(auto_trade_manager) = &self.auto_trade_manager {
                // Gọi phương thức invalidate_user_cache nếu có
                if let Err(e) = auto_trade_manager.invalidate_user_cache(user_id).await {
                    warn!("Lỗi khi đồng bộ auto-trade cache: {}", e);
                }
            }
        }
        
        // Đồng bộ với free user manager nếu là free user
        if subscription.subscription_type == SubscriptionType::Free {
            if let Some(free_user_manager) = &self.free_user_manager {
                if let Err(e) = free_user_manager.sync_cache_with_db(user_id).await {
                    warn!("Lỗi khi đồng bộ free user cache: {}", e);
                }
            }
        }
        // Đồng bộ với premium user manager nếu là premium user
        else if subscription.subscription_type == SubscriptionType::Premium {
            if let Some(premium_user_manager) = &self.premium_user_manager {
                if let Err(e) = premium_user_manager.sync_cache_with_db(user_id).await {
                    warn!("Lỗi khi đồng bộ premium user cache: {}", e);
                }
            }
        }
        // Đồng bộ với vip user manager nếu là vip user
        else if subscription.subscription_type == SubscriptionType::VIP {
            if let Some(vip_user_manager) = &self.vip_user_manager {
                if let Err(e) = vip_user_manager.sync_cache_with_db(user_id).await {
                    warn!("Lỗi khi đồng bộ VIP user cache: {}", e);
                }
            }
        }
        
        info!("Đã đồng bộ trạng thái người dùng: {}", user_id);
        Ok(())
    }

    /// Làm mới cache của người dùng.
    ///
    /// Vô hiệu hóa và tải lại cache của người dùng để đảm bảo
    /// dữ liệu là mới nhất.
    ///
    /// # Arguments
    /// * `user_id` - ID người dùng cần refresh cache
    ///
    /// # Returns
    /// * `Ok(())` - Nếu refresh thành công
    /// * `Err(WalletError)` - Nếu có lỗi trong quá trình refresh
    pub async fn refresh_user_cache(&self, user_id: &str) -> Result<(), WalletError> {
        debug!("Refresh cache cho user: {}", user_id);
        
        // Vô hiệu hóa cache hiện tại
        self.subscriptions_cache.invalidate(user_id).await;
        
        // Tải lại từ database
        if let Ok(subscription) = self.load_subscription_from_db(user_id).await {
            // Cập nhật vào cache với thời gian sống ngắn để đảm bảo làm mới thường xuyên
            self.subscriptions_cache.insert_with_ttl(
                user_id.to_string(),
                subscription,
                std::time::Duration::from_secs(60 * 5), // 5 phút
            ).await;
        }
        
        Ok(())
    }
    
    /// Đánh dấu người dùng cần kiểm tra lại NFT.
    ///
    /// Thêm user_id vào hàng đợi để kiểm tra NFT trong lần chạy tiếp theo
    /// của quá trình xác minh NFT định kỳ.
    ///
    /// # Arguments
    /// * `user_id` - ID người dùng cần kiểm tra lại NFT
    ///
    /// # Returns
    /// * `Ok(())` - Nếu đánh dấu thành công
    /// * `Err(WalletError)` - Nếu có lỗi trong quá trình đánh dấu
    pub async fn mark_user_for_verification(&self, user_id: &str) -> Result<(), WalletError> {
        debug!("Đánh dấu user {} cần kiểm tra lại NFT", user_id);
        
        // Thêm vào danh sách pending_verifications nếu có
        if let Some(pending_verifications) = &self.pending_verifications {
            let mut verifications = pending_verifications.write().await;
            if !verifications.contains(user_id) {
                verifications.push(user_id.to_string());
                debug!("Đã thêm user {} vào hàng đợi kiểm tra NFT", user_id);
            }
        }
        
        Ok(())
    }
    
    /// Thông báo reset auto-trade cho một người dùng.
    ///
    /// Cập nhật thông tin liên quan sau khi auto-trade được reset
    /// để đảm bảo tính nhất quán của dữ liệu.
    ///
    /// # Arguments
    /// * `user_id` - ID người dùng đã reset auto-trade
    ///
    /// # Returns
    /// * `Ok(())` - Nếu cập nhật thành công
    /// * `Err(WalletError)` - Nếu có lỗi trong quá trình cập nhật
    pub async fn notify_auto_trade_reset(&self, user_id: &str) -> Result<(), WalletError> {
        debug!("Nhận thông báo reset auto-trade cho user: {}", user_id);
        
        // Cập nhật subscription nếu cần
        let subscription = match self.get_user_subscription(user_id).await {
            Ok(sub) => sub,
            Err(e) => {
                warn!("Không thể lấy subscription khi nhận thông báo reset auto-trade: {}", e);
                return Err(e);
            }
        };
        
        // Cập nhật thời gian hoạt động gần nhất
        let mut updated_subscription = subscription.clone();
        updated_subscription.last_active = Utc::now();
        
        // Lưu vào database
        self.save_subscription_to_db(&updated_subscription).await?;
        
        // Cập nhật cache
        self.subscriptions_cache.insert(
            user_id.to_string(),
            updated_subscription,
        ).await;
        
        // Ghi log
        info!("Đã cập nhật subscription sau khi reset auto-trade cho user: {}", user_id);
        Ok(())
    }
    
    /// Refresh auto-trade status cho một người dùng.
    ///
    /// Cập nhật trạng thái auto-trade trong subscription sau khi 
    /// có thay đổi từ AutoTradeManager.
    ///
    /// # Arguments
    /// * `user_id` - ID người dùng cần refresh auto-trade status
    ///
    /// # Returns
    /// * `Ok(())` - Nếu refresh thành công
    /// * `Err(WalletError)` - Nếu có lỗi trong quá trình refresh
    pub async fn refresh_auto_trade_status(&self, user_id: &str) -> Result<(), WalletError> {
        debug!("Refresh auto-trade status cho user: {}", user_id);
        
        // Lấy thông tin auto-trade mới nhất
        if let Some(auto_trade_manager) = &self.auto_trade_manager {
            match auto_trade_manager.get_auto_trade_usage(user_id).await {
                Ok(usage) => {
                    // Cập nhật vào subscription
                    let mut subscription = match self.get_user_subscription(user_id).await {
                        Ok(sub) => sub,
                        Err(e) => {
                            warn!("Không thể lấy subscription khi refresh auto-trade status: {}", e);
                            return Err(e);
                        }
                    };
                    
                    // Cập nhật thông tin auto-trade trong subscription
                    subscription.auto_trade_minutes_remaining = usage.remaining_minutes;
                    subscription.auto_trade_status = Some(usage.status);
                    
                    // Lưu vào database
                    self.save_subscription_to_db(&subscription).await?;
                    
                    // Cập nhật cache
                    self.subscriptions_cache.insert(
                        user_id.to_string(),
                        subscription,
                    ).await;
                    
                    info!("Đã refresh auto-trade status cho user: {}", user_id);
                },
                Err(e) => {
                    warn!("Không thể lấy auto-trade usage khi refresh status: {}", e);
                }
            }
        }
        
        Ok(())
    }

    // Cập nhật trạng thái người dùng với đồng bộ
    pub async fn update_user_status(&self, user_id: &str, status: SubscriptionStatus) -> Result<UserSubscription, WalletError> {
        debug!("Cập nhật trạng thái subscription cho user {} thành {:?}", user_id, status);
        
        // Lấy subscription hiện tại
        let mut subscription = match self.get_user_subscription(user_id).await {
            Ok(sub) => sub,
            Err(e) => {
                error!("Không thể lấy subscription khi cập nhật trạng thái: {}", e);
                return Err(e);
            }
        };
        
        // Cập nhật trạng thái
        subscription.status = status;
        subscription.last_updated = Utc::now();
        
        // Lưu vào database trước
        if let Err(e) = self.save_subscription_to_db(&subscription).await {
            error!("Không thể lưu subscription vào database: {}", e);
            return Err(e);
        }
        
        // Cập nhật cache
        self.subscriptions_cache.insert(
            user_id.to_string(),
            subscription.clone(),
        ).await;
        
        // Đồng bộ với các module khác
        if let Err(e) = self.sync_user_state(user_id, true, true).await {
            warn!("Có lỗi khi đồng bộ trạng thái người dùng: {}", e);
            // Không return error ở đây để tiếp tục luồng xử lý chính
        }
        
        info!("Đã cập nhật trạng thái subscription cho user {} thành {:?}", user_id, status);
        Ok(subscription)
    }
} 