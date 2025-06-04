//! Tests cho module subscription

#[cfg(test)]
mod tests {
    use super::super::*;
    use chrono::{Duration, Utc};
    use ethers::types::{Address, U256};
    use mockall::predicate::*;
    use uuid::Uuid;
    use super::super::staking::{StakingManager, TokenStake, StakeStatus, StakingError};
    use super::super::nft::{NftInfo, VipNftInfo, NonNftVipStatus};
    use crate::walletmanager::chain::ChainType;
    use crate::walletmanager::api::WalletManagerApi;
    use crate::walletmanager::types::WalletSystemConfig;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use std::str::FromStr;

    // Test cho user_subscription
    #[test]
    fn test_user_subscription_is_active() {
        let user_id = "test_user";
        let payment_address = Address::random();
        let payment_token = PaymentToken::USDC;
        let payment_amount = U256::from(100);
        
        let mut subscription = UserSubscription::new(
            user_id,
            SubscriptionType::Premium,
            payment_address,
            payment_token,
            30, // 30 days
            payment_amount,
        );
        
        // Subscription is not active before payment
        assert!(!subscription.is_active());
        
        // Activate subscription
        subscription.activate("0x1234567890abcdef");
        assert!(subscription.is_active());
        
        // Test expiration
        subscription.end_date = Utc::now() - Duration::days(1);
        assert!(!subscription.is_active());
        assert!(subscription.is_expired());
    }
    
    #[test]
    fn test_user_subscription_extend() {
        let user_id = "test_user";
        let payment_address = Address::random();
        let payment_token = PaymentToken::USDC;
        let payment_amount = U256::from(100);
        
        let mut subscription = UserSubscription::new(
            user_id,
            SubscriptionType::Premium,
            payment_address,
            payment_token,
            30, // 30 days
            payment_amount,
        );
        
        let original_end_date = subscription.end_date;
        
        // Extend by 30 days
        subscription.extend(30);
        
        // Check if end_date is extended by 30 days
        let expected_end_date = original_end_date + Duration::days(30);
        assert_eq!(subscription.end_date, expected_end_date);
        
        // Set to expired and extend
        subscription.status = SubscriptionStatus::Expired;
        subscription.end_date = Utc::now() - Duration::days(1);
        subscription.extend(30);
        
        // Check if status is reactivated
        assert_eq!(subscription.status, SubscriptionStatus::Active);
    }
    
    // Test cho chức năng staking
    #[tokio::test]
    async fn test_token_stake_creation_and_completion() {
        // Setup
        let user_id = "test_staking_user";
        let wallet_address = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();
        let chain_type = ChainType::EVM;
        let amount = U256::from(1000);
        let duration_days = 365;
        
        // Tạo token stake mới
        let mut stake = TokenStake::new(
            user_id,
            wallet_address,
            chain_type,
            amount,
            duration_days,
        );
        
        // Kiểm tra các trường cơ bản
        assert_eq!(stake.user_id, user_id);
        assert_eq!(stake.wallet_address, wallet_address);
        assert_eq!(stake.amount, amount);
        assert_eq!(stake.status, StakeStatus::Pending);
        assert!(stake.stake_tx_hash.is_none());
        assert!(stake.unstake_tx_hash.is_none());
        
        // Cập nhật stake transaction hash
        let stake_tx_hash = "0xabcdef1234567890abcdef1234567890";
        stake.set_stake_tx_hash(stake_tx_hash);
        assert_eq!(stake.stake_tx_hash, Some(stake_tx_hash.to_string()));
        assert_eq!(stake.status, StakeStatus::Pending); // Vẫn còn pending
        
        // Hoàn tất stake
        let unstake_tx_hash = "0x9876543210fedcba9876543210fedcba";
        let complete_result = stake.complete(unstake_tx_hash);
        assert!(complete_result.is_ok());
        assert_eq!(stake.status, StakeStatus::Completed);
        assert_eq!(stake.unstake_tx_hash, Some(unstake_tx_hash.to_string()));
        
        // Kiểm tra các phương thức khác
        assert!(!stake.is_expired()); // đã completed nên không còn active
        
        // Thử đánh dấu stake đã completed là VIP subscription
        stake.mark_as_vip_subscription();
        assert!(stake.is_vip_subscription());
    }
    
    #[tokio::test]
    async fn test_staking_manager() {
        // Setup mocked wallet API
        let config = WalletSystemConfig::default();
        let wallet_api = Arc::new(RwLock::new(WalletManagerApi::new(config)));
        
        // Tạo staking manager
        let staking_manager = StakingManager::new(wallet_api);
        
        // Thiết lập test data
        let user_id = "staking_test_user";
        let wallet_address = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();
        let chain_type = ChainType::EVM;
        let amount = U256::from(1000);
        
        // Test các chức năng của StakingManager
        // Chú ý: Đây là những mock test, thực tế các chức năng sẽ tương tác với database và blockchain
        
        // Test days_remaining
        let mut stake = TokenStake::new(
            user_id,
            wallet_address,
            chain_type,
            amount,
            365,
        );
        
        let days = StakingManager::days_remaining(&stake);
        assert!(days > 0 && days <= 365, "Số ngày còn lại phải trong khoảng (0, 365]: {}", days);
        
        // Test với stake đã hết hạn
        stake.end_date = Utc::now() - Duration::days(1);
        let days = StakingManager::days_remaining(&stake);
        assert_eq!(days, 0, "Stake đã hết hạn phải có số ngày còn lại là 0");
        
        // Test với stake đã hoàn thành
        stake.end_date = Utc::now() + Duration::days(100);
        stake.status = StakeStatus::Completed;
        let days = StakingManager::days_remaining(&stake);
        assert_eq!(days, 0, "Stake đã hoàn thành phải có số ngày còn lại là 0");
    }
    
    // Test cho chức năng verify NFT
    #[tokio::test]
    async fn test_nft_verification() {
        // Tạo NftInfo
        let contract_address = Address::from_str("0xabcdef1234567890abcdef1234567890abcdef12").unwrap();
        let token_id = U256::from(12345);
        let mut nft_info = NftInfo::new(contract_address, token_id);
        
        // Kiểm tra trạng thái ban đầu
        assert!(nft_info.is_valid);
        assert!(!nft_info.needs_verification(Utc::now())); // Mới tạo nên chưa cần verify
        
        // Cập nhật trạng thái
        nft_info.update_status(false, Utc::now());
        assert!(!nft_info.is_valid);
        
        // Kiểm tra nếu cần verify sau 24h
        let future_time = Utc::now() + Duration::hours(25);
        assert!(nft_info.needs_verification(future_time));
        
        // Tạo VipNftInfo
        let user_id = Uuid::new_v4();
        let wallet_address = "0x1234567890123456789012345678901234567890";
        let vip_nft = VipNftInfo::new(user_id, wallet_address.to_string(), nft_info.clone());
        
        // Kiểm tra trạng thái ban đầu của VipNftInfo
        assert!(!vip_nft.is_active()); // Chưa activated
        
        // Kích hoạt NFT
        let activate_result = vip_nft.activate();
        assert!(activate_result.is_err()); // Nên thất bại vì NFT không hợp lệ
    }
    
    // Có thể bổ sung thêm nhiều test khác tùy theo nhu cầu
} 