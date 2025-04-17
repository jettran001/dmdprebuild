//! Module validator cho proof-of-stake
//!
//! Module này cung cấp các chức năng:
//! - Quản lý validator
//! - Xác thực giao dịch
//! - Phân phối rewards cho validator

use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;
use async_trait::async_trait;
use ethers::types::{Address, U256};
use tracing::{info, warn, error, debug};

use crate::stake::{StakePoolConfig, UserStakeInfo};
use super::constants::*;

/// Thông tin validator
#[derive(Debug, Clone)]
pub struct ValidatorInfo {
    /// Địa chỉ validator
    pub address: Address,
    /// Số lượng token đã stake
    pub staked_amount: U256,
    /// Thời gian bắt đầu làm validator
    pub start_time: u64,
    /// Số block đã xác thực
    pub blocks_validated: u64,
    /// Rewards đã nhận
    pub rewards_earned: U256,
    /// Trạng thái hoạt động
    pub is_active: bool,
}

/// Trait cho validator
#[async_trait]
pub trait Validator: Send + Sync {
    /// Đăng ký làm validator
    async fn register_validator(&self, pool_address: Address, validator_address: Address) -> Result<()>;
    
    /// Hủy đăng ký validator
    async fn unregister_validator(&self, pool_address: Address, validator_address: Address) -> Result<()>;
    
    /// Xác thực giao dịch
    async fn validate_transaction(&self, pool_address: Address, transaction_hash: &str) -> Result<bool>;
    
    /// Lấy danh sách validator
    async fn get_validators(&self, pool_address: Address) -> Result<Vec<ValidatorInfo>>;
    
    /// Lấy thông tin validator
    async fn get_validator_info(&self, pool_address: Address, validator_address: Address) -> Result<ValidatorInfo>;
    
    /// Phân phối rewards cho validator
    async fn distribute_validator_rewards(&self, pool_address: Address) -> Result<()>;
}

/// Implement Validator
pub struct ValidatorImpl {
    /// Cache cho validators
    validators: Arc<RwLock<Vec<ValidatorInfo>>>,
}

impl ValidatorImpl {
    /// Tạo validator mới
    pub fn new() -> Self {
        Self {
            validators: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

#[async_trait]
impl Validator for ValidatorImpl {
    async fn register_validator(&self, pool_address: Address, validator_address: Address) -> Result<()> {
        // Kiểm tra địa chỉ hợp lệ
        if validator_address == Address::zero() {
            return Err(anyhow::anyhow!("Invalid validator address: zero address"));
        }
        
        if pool_address == Address::zero() {
            return Err(anyhow::anyhow!("Invalid pool address: zero address"));
        }
        
        // Kiểm tra quyền hạn dựa trên stake amount
        let min_stake_required = match self.stake_manager.get_min_stake_for_validator(pool_address).await {
            Ok(amount) => amount,
            Err(e) => {
                error!("Failed to get minimum stake requirement: {:?}", e);
                return Err(anyhow::anyhow!("Could not verify stake requirements: {}", e));
            }
        };
        
        let validator_stake = match self.stake_manager.get_user_stake_amount(pool_address, validator_address).await {
            Ok(amount) => amount,
            Err(e) => {
                error!("Failed to get validator stake amount: {:?}", e);
                return Err(anyhow::anyhow!("Could not verify validator stake: {}", e));
            }
        };
        
        if validator_stake < min_stake_required {
            return Err(anyhow::anyhow!(
                "Insufficient stake amount. Required: {}, Current: {}", 
                min_stake_required, validator_stake
            ));
        }
        
        // Kiểm tra thời gian lock của stake
        let min_lock_time = match self.stake_manager.get_min_lock_time_for_validator(pool_address).await {
            Ok(time) => time,
            Err(e) => {
                error!("Failed to get minimum lock time requirement: {:?}", e);
                return Err(anyhow::anyhow!("Could not verify lock time requirements: {}", e));
            }
        };
        
        let validator_lock_time = match self.stake_manager.get_user_lock_time(pool_address, validator_address).await {
            Ok(time) => time,
            Err(e) => {
                error!("Failed to get validator lock time: {:?}", e);
                return Err(anyhow::anyhow!("Could not verify validator lock time: {}", e));
            }
        };
        
        if validator_lock_time < min_lock_time {
            return Err(anyhow::anyhow!(
                "Insufficient lock time. Required: {} seconds, Current: {} seconds", 
                min_lock_time, validator_lock_time
            ));
        }
        
        // Kiểm tra lịch sử hoạt động của validator (nếu đã từng là validator trước đó)
        let validator_history = match self.get_validator_history(validator_address).await {
            Ok(history) => history,
            Err(_) => {
                debug!("No previous validator history found for: {:?}", validator_address);
                None
            }
        };
        
        if let Some(history) = validator_history {
            // Kiểm tra nếu validator đã từng bị phạt hoặc bị cấm
            if history.is_banned {
                return Err(anyhow::anyhow!("Validator is banned from the network"));
            }
            
            if history.penalty_count > MAX_ALLOWED_PENALTIES {
                return Err(anyhow::anyhow!(
                    "Validator has too many penalties: {}. Maximum allowed: {}", 
                    history.penalty_count, MAX_ALLOWED_PENALTIES
                ));
            }
        }
        
        // Kiểm tra số lượng validator hiện tại
        let mut validators = self.validators.write().await;
        
        // Kiểm tra số lượng validator tối đa
        if validators.len() >= MAX_VALIDATORS as usize {
            return Err(anyhow::anyhow!("Maximum number of validators reached"));
        }
        
        // Kiểm tra validator đã tồn tại
        if validators.iter().any(|v| v.address == validator_address) {
            return Err(anyhow::anyhow!("Validator already registered"));
        }
        
        // Thực hiện giao dịch để đăng ký validator trên blockchain
        match self.router.register_validator(pool_address, validator_address).await {
            Ok(tx_hash) => {
                info!("Validator registration transaction sent: {:?}", tx_hash);
            },
            Err(e) => {
                error!("Failed to send validator registration transaction: {:?}", e);
                return Err(anyhow::anyhow!("Failed to register validator on blockchain: {}", e));
            }
        }
        
        // Thêm validator mới vào danh sách
        validators.push(ValidatorInfo {
            address: validator_address,
            staked_amount: validator_stake,
            start_time: Self::get_current_time(),
            blocks_validated: 0,
            rewards_earned: U256::zero(),
            is_active: true,
        });
        
        info!("Validator registered: {:?}", validator_address);
        Ok(())
    }

    async fn unregister_validator(&self, pool_address: Address, validator_address: Address) -> Result<()> {
        let mut validators = self.validators.write().await;
        
        // Kiểm tra số lượng validator tối thiểu
        if validators.len() <= MIN_VALIDATORS as usize {
            return Err(anyhow::anyhow!("Cannot unregister validator: minimum number required"));
        }
        
        // Tìm và xóa validator
        if let Some(index) = validators.iter().position(|v| v.address == validator_address) {
            validators.remove(index);
            info!("Validator unregistered: {:?}", validator_address);
            Ok(())
        } else {
            Err(anyhow::anyhow!("Validator not found"))
        }
    }

    async fn validate_transaction(&self, pool_address: Address, transaction_hash: &str) -> Result<bool> {
        // TODO: Implement logic xác thực giao dịch
        // 1. Kiểm tra chữ ký
        // 2. Kiểm tra nonce
        // 3. Kiểm tra gas
        // 4. Kiểm tra balance
        Ok(true)
    }

    async fn get_validators(&self, pool_address: Address) -> Result<Vec<ValidatorInfo>> {
        let validators = self.validators.read().await;
        Ok(validators.clone())
    }

    async fn get_validator_info(&self, pool_address: Address, validator_address: Address) -> Result<ValidatorInfo> {
        let validators = self.validators.read().await;
        if let Some(validator) = validators.iter().find(|v| v.address == validator_address) {
            Ok(validator.clone())
        } else {
            Err(anyhow::anyhow!("Validator not found"))
        }
    }

    async fn distribute_validator_rewards(&self, pool_address: Address) -> Result<()> {
        // TODO: Implement logic phân phối rewards cho validator
        // 1. Tính toán rewards dựa trên số block đã xác thực
        // 2. Phân phối rewards theo tỷ lệ stake
        // 3. Cập nhật thông tin validator
        Ok(())
    }
}
