//! Module quản lý token pairs và liquidity pools
//!
//! Module này cung cấp các chức năng để quản lý token pairs và liquidity pools trên DEX.
//! Hiện tại đang trong giai đoạn phát triển ban đầu.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use log::{info, debug, warn, error};
use anyhow::{Result, Context};

/// Lỗi có thể xảy ra trong quá trình quản lý pairs
#[derive(Error, Debug)]
pub enum PairError {
    #[error("Pair không tồn tại: {0}")]
    PairNotFound(String),
    
    #[error("Token không tồn tại: {0}")]
    TokenNotFound(String),
    
    #[error("Liquidity không đủ: yêu cầu {required}, hiện có {available}")]
    InsufficientLiquidity {
        required: f64,
        available: f64,
    },
    
    #[error("Slippage vượt quá giới hạn: {actual}% > {max}%")]
    SlippageExceeded {
        actual: f64,
        max: f64,
    },
    
    #[error("Lỗi hệ thống: {0}")]
    SystemError(String),
}

/// Kết quả của các hoạt động pair
pub type PairResult<T> = Result<T, PairError>;

/// Thông tin về token pair
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenPair {
    /// ID của pair
    pub id: String,
    
    /// Địa chỉ của token 1
    pub token1_address: String,
    
    /// Địa chỉ của token 2
    pub token2_address: String,
    
    /// Tên của pair (VD: ETH-USDT)
    pub name: String,
    
    /// Tỷ lệ token 1 / token 2
    pub exchange_rate: f64,
    
    /// Tổng liquidity của token 1
    pub token1_liquidity: f64,
    
    /// Tổng liquidity của token 2
    pub token2_liquidity: f64,
    
    /// Địa chỉ của pool contract
    pub pool_address: Option<String>,
    
    /// Thời gian tạo pair
    pub created_at: u64,
    
    /// Thời gian cập nhật cuối cùng
    pub updated_at: u64,
}

/// Quản lý token pairs
#[derive(Debug)]
pub struct PairManager {
    /// Các token pairs, key là pair ID
    pairs: HashMap<String, TokenPair>,
    
    /// ID counter cho pairs
    pair_id_counter: u64,
}

impl PairManager {
    /// Tạo một PairManager mới
    pub fn new() -> Self {
        Self {
            pairs: HashMap::new(),
            pair_id_counter: 0,
        }
    }
    
    /// Tạo một token pair mới
    pub fn create_pair(&mut self, token1_address: &str, token2_address: &str, name: &str) -> PairResult<String> {
        let now = get_current_timestamp();
        let id = format!("pair_{}", self.pair_id_counter);
        self.pair_id_counter += 1;
        
        let pair = TokenPair {
            id: id.clone(),
            token1_address: token1_address.to_string(),
            token2_address: token2_address.to_string(),
            name: name.to_string(),
            exchange_rate: 1.0, // Tỷ lệ mặc định
            token1_liquidity: 0.0,
            token2_liquidity: 0.0,
            pool_address: None,
            created_at: now,
            updated_at: now,
        };
        
        self.pairs.insert(id.clone(), pair);
        
        info!("Token pair mới được tạo: {} ({}-{})", id, token1_address, token2_address);
        Ok(id)
    }
    
    /// Lấy thông tin về token pair
    pub fn get_pair(&self, pair_id: &str) -> PairResult<&TokenPair> {
        self.pairs.get(pair_id)
            .ok_or_else(|| PairError::PairNotFound(pair_id.to_string()))
    }
    
    /// Thêm liquidity vào pair
    pub fn add_liquidity(&mut self, pair_id: &str, token1_amount: f64, token2_amount: f64) -> PairResult<()> {
        if token1_amount <= 0.0 || token2_amount <= 0.0 {
            return Err(PairError::InsufficientLiquidity {
                required: token1_amount.max(token2_amount),
                available: 0.0,
            });
        }
        
        let pair = self.pairs.get_mut(pair_id)
            .ok_or_else(|| PairError::PairNotFound(pair_id.to_string()))?;
            
        pair.token1_liquidity += token1_amount;
        pair.token2_liquidity += token2_amount;
        
        // Cập nhật tỷ lệ trao đổi nếu có liquidity đầy đủ
        if pair.token1_liquidity > 0.0 && pair.token2_liquidity > 0.0 {
            pair.exchange_rate = pair.token1_liquidity / pair.token2_liquidity;
        }
        
        pair.updated_at = get_current_timestamp();
        
        info!("Đã thêm liquidity ({}, {}) vào pair {}", token1_amount, token2_amount, pair_id);
        Ok(())
    }
    
    /// Rút liquidity từ pair
    pub fn remove_liquidity(&mut self, pair_id: &str, token1_amount: f64, token2_amount: f64) -> PairResult<()> {
        let pair = self.pairs.get_mut(pair_id)
            .ok_or_else(|| PairError::PairNotFound(pair_id.to_string()))?;
            
        if token1_amount > pair.token1_liquidity {
            return Err(PairError::InsufficientLiquidity {
                required: token1_amount,
                available: pair.token1_liquidity,
            });
        }
        
        if token2_amount > pair.token2_liquidity {
            return Err(PairError::InsufficientLiquidity {
                required: token2_amount,
                available: pair.token2_liquidity,
            });
        }
        
        pair.token1_liquidity -= token1_amount;
        pair.token2_liquidity -= token2_amount;
        
        // Cập nhật tỷ lệ trao đổi nếu còn liquidity
        if pair.token1_liquidity > 0.0 && pair.token2_liquidity > 0.0 {
            pair.exchange_rate = pair.token1_liquidity / pair.token2_liquidity;
        }
        
        pair.updated_at = get_current_timestamp();
        
        info!("Đã rút liquidity ({}, {}) từ pair {}", token1_amount, token2_amount, pair_id);
        Ok(())
    }
    
    /// Swap token 1 sang token 2
    pub fn swap_token1_to_token2(&mut self, pair_id: &str, token1_amount: f64, max_slippage: f64) -> PairResult<f64> {
        let pair = self.pairs.get_mut(pair_id)
            .ok_or_else(|| PairError::PairNotFound(pair_id.to_string()))?;
            
        if token1_amount <= 0.0 {
            return Err(PairError::InsufficientLiquidity {
                required: token1_amount,
                available: 0.0,
            });
        }
        
        if token1_amount > pair.token1_liquidity * 0.5 {
            // Kiểm tra slippage
            let k_before = pair.token1_liquidity * pair.token2_liquidity;
            let new_token1 = pair.token1_liquidity + token1_amount;
            let new_token2 = k_before / new_token1;
            let token2_out = pair.token2_liquidity - new_token2;
            
            let expected_out = token1_amount / pair.exchange_rate;
            let slippage = (expected_out - token2_out) / expected_out * 100.0;
            
            if slippage > max_slippage {
                return Err(PairError::SlippageExceeded {
                    actual: slippage,
                    max: max_slippage,
                });
            }
            
            // Thực hiện swap
            pair.token1_liquidity = new_token1;
            pair.token2_liquidity = new_token2;
            pair.exchange_rate = new_token1 / new_token2;
            pair.updated_at = get_current_timestamp();
            
            info!("Đã swap {} token1 thành {} token2 trong pair {}", 
                token1_amount, token2_out, pair_id);
                
            Ok(token2_out)
        } else {
            // Swap nhỏ, không ảnh hưởng nhiều đến giá
            let token2_out = token1_amount / pair.exchange_rate;
            
            pair.token1_liquidity += token1_amount;
            pair.token2_liquidity -= token2_out;
            pair.updated_at = get_current_timestamp();
            
            info!("Đã swap {} token1 thành {} token2 trong pair {}", 
                token1_amount, token2_out, pair_id);
                
            Ok(token2_out)
        }
    }
    
    /// Lấy tất cả token pairs
    pub fn get_all_pairs(&self) -> Vec<&TokenPair> {
        self.pairs.values().collect()
    }
    
    /// Tìm pairs theo token
    pub fn find_pairs_by_token(&self, token_address: &str) -> Vec<&TokenPair> {
        self.pairs.values()
            .filter(|pair| pair.token1_address == token_address || pair.token2_address == token_address)
            .collect()
    }
    
    /// Cập nhật địa chỉ pool contract
    pub fn set_pool_address(&mut self, pair_id: &str, pool_address: &str) -> PairResult<()> {
        let pair = self.pairs.get_mut(pair_id)
            .ok_or_else(|| PairError::PairNotFound(pair_id.to_string()))?;
            
        pair.pool_address = Some(pool_address.to_string());
        pair.updated_at = get_current_timestamp();
        
        info!("Đã cập nhật địa chỉ pool contract cho pair {}: {}", pair_id, pool_address);
        Ok(())
    }
}

/// Lấy timestamp hiện tại dưới dạng giây
fn get_current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(std::time::Duration::from_secs(0))
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_create_and_get_pair() {
        let mut manager = PairManager::new();
        
        let token1 = "0xToken1";
        let token2 = "0xToken2";
        let name = "TOKEN1-TOKEN2";
        
        let pair_id = manager.create_pair(token1, token2, name).unwrap();
        
        let pair = manager.get_pair(&pair_id).unwrap();
        assert_eq!(pair.token1_address, token1);
        assert_eq!(pair.token2_address, token2);
        assert_eq!(pair.name, name);
        assert_eq!(pair.token1_liquidity, 0.0);
        assert_eq!(pair.token2_liquidity, 0.0);
        assert_eq!(pair.exchange_rate, 1.0);
    }
    
    #[test]
    fn test_add_and_remove_liquidity() {
        let mut manager = PairManager::new();
        
        let pair_id = manager.create_pair("0xToken1", "0xToken2", "TOKEN1-TOKEN2").unwrap();
        
        // Thêm liquidity
        manager.add_liquidity(&pair_id, 100.0, 200.0).unwrap();
        
        let pair = manager.get_pair(&pair_id).unwrap();
        assert_eq!(pair.token1_liquidity, 100.0);
        assert_eq!(pair.token2_liquidity, 200.0);
        assert_eq!(pair.exchange_rate, 0.5); // 100/200 = 0.5
        
        // Thêm thêm liquidity
        manager.add_liquidity(&pair_id, 50.0, 100.0).unwrap();
        
        let pair = manager.get_pair(&pair_id).unwrap();
        assert_eq!(pair.token1_liquidity, 150.0);
        assert_eq!(pair.token2_liquidity, 300.0);
        assert_eq!(pair.exchange_rate, 0.5); // 150/300 = 0.5
        
        // Rút liquidity
        manager.remove_liquidity(&pair_id, 50.0, 100.0).unwrap();
        
        let pair = manager.get_pair(&pair_id).unwrap();
        assert_eq!(pair.token1_liquidity, 100.0);
        assert_eq!(pair.token2_liquidity, 200.0);
        assert_eq!(pair.exchange_rate, 0.5); // 100/200 = 0.5
    }
    
    #[test]
    fn test_swap_tokens() {
        let mut manager = PairManager::new();
        
        let pair_id = manager.create_pair("0xToken1", "0xToken2", "TOKEN1-TOKEN2").unwrap();
        
        // Thêm liquidity
        manager.add_liquidity(&pair_id, 1000.0, 2000.0).unwrap();
        
        // Swap nhỏ
        let token2_amount = manager.swap_token1_to_token2(&pair_id, 10.0, 1.0).unwrap();
        assert!(token2_amount > 19.0 && token2_amount < 21.0); // Khoảng 20.0
        
        let pair = manager.get_pair(&pair_id).unwrap();
        assert_eq!(pair.token1_liquidity, 1010.0);
        assert!(pair.token2_liquidity < 2000.0 && pair.token2_liquidity > 1980.0);
    }
    
    #[test]
    fn test_find_pairs_by_token() {
        let mut manager = PairManager::new();
        
        let token1 = "0xToken1";
        let token2 = "0xToken2";
        let token3 = "0xToken3";
        
        manager.create_pair(token1, token2, "TOKEN1-TOKEN2").unwrap();
        manager.create_pair(token1, token3, "TOKEN1-TOKEN3").unwrap();
        manager.create_pair(token2, token3, "TOKEN2-TOKEN3").unwrap();
        
        let pairs = manager.find_pairs_by_token(token1);
        assert_eq!(pairs.len(), 2);
        
        let pairs = manager.find_pairs_by_token(token2);
        assert_eq!(pairs.len(), 2);
        
        let pairs = manager.find_pairs_by_token(token3);
        assert_eq!(pairs.len(), 2);
        
        let pairs = manager.find_pairs_by_token("0xNonExistent");
        assert_eq!(pairs.len(), 0);
    }
    
    #[test]
    fn test_error_handling() {
        let mut manager = PairManager::new();
        
        // Pair không tồn tại
        let result = manager.get_pair("non-existent-pair");
        assert!(matches!(result, Err(PairError::PairNotFound(_))));
        
        // Thêm liquidity cho pair không tồn tại
        let result = manager.add_liquidity("non-existent-pair", 100.0, 200.0);
        assert!(matches!(result, Err(PairError::PairNotFound(_))));
        
        // Tạo pair
        let pair_id = manager.create_pair("0xToken1", "0xToken2", "TOKEN1-TOKEN2").unwrap();
        
        // Thêm liquidity âm
        let result = manager.add_liquidity(&pair_id, -100.0, 200.0);
        assert!(matches!(result, Err(PairError::InsufficientLiquidity { .. })));
        
        // Rút quá nhiều liquidity
        let result = manager.remove_liquidity(&pair_id, 100.0, 200.0);
        assert!(matches!(result, Err(PairError::InsufficientLiquidity { .. })));
    }
}
