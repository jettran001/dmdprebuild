//! Tests cho module subscription

#[cfg(test)]
mod tests {
    use super::super::*;
    use chrono::{Duration, Utc};
    use ethers::types::{Address, U256};
    use mockall::predicate::*;
    use uuid::Uuid;

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
    
    // Test cho các chức năng của SubscriptionManager
    // Cần mock các dependencies như WalletManagerApi, FreeUserManager, PremiumUserManager
    
    // Có thể bổ sung thêm nhiều test khác tùy theo nhu cầu
} 