// Factory cho các farm pools
// 
// Factory tạo và quản lý các farm pools dựa trên các cấu hình khác nhau.
// Module này cung cấp các chức năng:
// - Tạo farm pools mới với các token khác nhau
// - Quản lý tập trung các farm pools
// - Cập nhật APR và các thông số khác
// - Đồng bộ dữ liệu với blockchain

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::{Result, Context};
use log::{info, warn, error, debug};
use tokio::time::{sleep, Duration};

use crate::farm::farm_logic::{FarmManager, FarmPoolConfig, FarmError};

/// Factory để tạo và quản lý farm pools
pub struct FarmFactory {
    /// Các farm managers, phân chia theo chain ID
    managers: RwLock<HashMap<String, Arc<RwLock<FarmManager>>>>,
    /// Cấu hình mặc định cho farm pools mới
    default_apr: f64,
    /// Backup của dữ liệu farm pools
    backup_data: RwLock<HashMap<String, Vec<FarmPoolConfig>>>,
    /// Số lần retry tối đa
    max_retries: u32,
    /// Thời gian chờ giữa các lần retry (ms)
    retry_delay_ms: u64,
}

impl FarmFactory {
    /// Tạo factory mới với APR mặc định
    pub fn new(default_apr: f64) -> Self {
        Self {
            managers: RwLock::new(HashMap::new()),
            default_apr,
            backup_data: RwLock::new(HashMap::new()),
            max_retries: 3,
            retry_delay_ms: 1000,
        }
    }
    
    /// Tạo factory với cấu hình nâng cao
    pub fn with_config(default_apr: f64, max_retries: u32, retry_delay_ms: u64) -> Self {
        Self {
            managers: RwLock::new(HashMap::new()),
            default_apr,
            backup_data: RwLock::new(HashMap::new()),
            max_retries,
            retry_delay_ms,
        }
    }
    
    /// Lấy hoặc tạo farm manager cho chain cụ thể
    pub async fn get_or_create_manager(&self, chain_id: &str) -> Result<Arc<RwLock<FarmManager>>> {
        let mut managers = self.managers.write().await;
        
        if !managers.contains_key(chain_id) {
            info!("Tạo farm manager mới cho chain: {}", chain_id);
            let manager = Arc::new(RwLock::new(FarmManager::new()));
            managers.insert(chain_id.to_string(), manager.clone());
            return Ok(manager);
        }
        
        Ok(managers.get(chain_id).unwrap().clone())
    }
    
    /// Tạo pool mới trên chain đã chỉ định
    pub async fn create_pool(&self, chain_id: &str, pair_name: &str, apr: Option<f64>) -> Result<String> {
        let manager = self.get_or_create_manager(chain_id).await?;
        let mut manager_lock = manager.write().await;
        
        let apr_value = apr.unwrap_or(self.default_apr);
        let pool_id = manager_lock.add_pool(pair_name, apr_value)
            .map_err(|e| anyhow::anyhow!("Không thể tạo pool: {}", e))?;
            
        info!("Đã tạo pool mới {} cho pair {} trên chain {}", pool_id, pair_name, chain_id);
        
        // Backup dữ liệu sau khi tạo pool
        self.backup_pool_data(chain_id, &manager_lock).await;
        
        Ok(pool_id)
    }
    
    /// Tạo các pools mặc định cho token cụ thể
    pub async fn create_default_pools(&self, chain_id: &str, token_address: &str) -> Result<Vec<String>> {
        let manager = self.get_or_create_manager(chain_id).await?;
        
        // Tạo các pairs mặc định
        let pairs = vec![
            format!("{}-USDT", token_address),
            format!("{}-ETH", token_address),
            format!("{}-BNB", token_address),
        ];
        
        let mut pool_ids = Vec::new();
        
        for (i, pair) in pairs.iter().enumerate() {
            let apr = self.default_apr + (i as f64 * 5.0); // Tăng dần APR cho mỗi pair
            let pool_id = self.create_pool(chain_id, pair, Some(apr)).await?;
            pool_ids.push(pool_id);
        }
        
        info!("Đã tạo {} pools mặc định cho token {} trên chain {}", 
              pool_ids.len(), token_address, chain_id);
              
        Ok(pool_ids)
    }
    
    /// Cập nhật APR cho pool cụ thể
    pub async fn update_pool_apr(&self, chain_id: &str, pool_id: &str, new_apr: f64) -> Result<()> {
        let manager = self.get_or_create_manager(chain_id).await?;
        let mut manager_lock = manager.write().await;
        
        manager_lock.update_apr(pool_id, new_apr)
            .map_err(|e| anyhow::anyhow!("Không thể cập nhật APR: {}", e))?;
            
        info!("Đã cập nhật APR cho pool {} trên chain {} thành {}", pool_id, chain_id, new_apr);
        
        // Backup dữ liệu sau khi cập nhật
        self.backup_pool_data(chain_id, &manager_lock).await;
        
        Ok(())
    }
    
    /// Backup dữ liệu pool của một chain
    async fn backup_pool_data(&self, chain_id: &str, manager: &FarmManager) {
        match manager.get_all_pools() {
            Ok(pools) => {
                let mut backups = self.backup_data.write().await;
                backups.insert(chain_id.to_string(), pools);
                debug!("Đã backup dữ liệu cho {} pools trên chain {}", pools.len(), chain_id);
            },
            Err(e) => {
                error!("Không thể backup dữ liệu pools cho chain {}: {}", chain_id, e);
            }
        }
    }
    
    /// Khôi phục dữ liệu từ backup
    async fn restore_from_backup(&self, chain_id: &str) -> Result<()> {
        let backups = self.backup_data.read().await;
        
        if let Some(pool_configs) = backups.get(chain_id) {
            let manager = self.get_or_create_manager(chain_id).await?;
            let mut manager_lock = manager.write().await;
            
            for config in pool_configs {
                if let Err(e) = manager_lock.restore_pool(config.clone()) {
                    warn!("Không thể khôi phục pool {}: {}", config.id, e);
                }
            }
            
            info!("Đã khôi phục dữ liệu cho chain {} từ backup", chain_id);
            Ok(())
        } else {
            warn!("Không có backup dữ liệu cho chain {}", chain_id);
            Err(anyhow::anyhow!("Không có backup dữ liệu"))
        }
    }
    
    /// Đồng bộ tất cả pools từ blockchain với cơ chế retry và backup
    pub async fn sync_all_pools(&self) -> Result<()> {
        let managers = self.managers.read().await;
        
        for (chain_id, manager) in managers.iter() {
            debug!("Bắt đầu đồng bộ pools cho chain {}", chain_id);
            
            let mut retry_count = 0;
            let mut success = false;
            
            // Thử đồng bộ với số lần retry cấu hình
            while retry_count < self.max_retries && !success {
                if retry_count > 0 {
                    debug!("Thử lại lần {}/{} cho chain {}", 
                          retry_count + 1, self.max_retries, chain_id);
                    sleep(Duration::from_millis(self.retry_delay_ms)).await;
                }
                
                let mut manager_lock = manager.write().await;
                
                match manager_lock.sync_pools_from_blockchain().await {
                    Ok(_) => {
                        info!("Đồng bộ thành công pools cho chain {}", chain_id);
                        
                        // Backup dữ liệu sau khi đồng bộ thành công
                        self.backup_pool_data(chain_id, &manager_lock).await;
                        
                        success = true;
                    },
                    Err(e) => {
                        error!("Lỗi khi đồng bộ pools cho chain {} (lần thử {}): {}", 
                              chain_id, retry_count + 1, e);
                        retry_count += 1;
                    }
                }
            }
            
            // Nếu đồng bộ thất bại sau khi đã retry, khôi phục từ backup
            if !success {
                warn!("Đồng bộ thất bại sau {} lần thử cho chain {}, khôi phục từ backup", 
                     self.max_retries, chain_id);
                if let Err(e) = self.restore_from_backup(chain_id).await {
                    error!("Không thể khôi phục từ backup cho chain {}: {}", chain_id, e);
                }
            }
        }
        
        info!("Đã hoàn thành đồng bộ tất cả farm pools");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_create_and_backup_pool() {
        let factory = FarmFactory::new(10.0);
        let chain_id = "test-chain";
        let pair_name = "TEST-USDT";
        
        // Tạo pool
        let pool_id = factory.create_pool(chain_id, pair_name, None).await.unwrap();
        
        // Kiểm tra backup
        let backups = factory.backup_data.read().await;
        assert!(backups.contains_key(chain_id));
        
        let chain_backups = backups.get(chain_id).unwrap();
        assert_eq!(chain_backups.len(), 1);
        assert_eq!(chain_backups[0].pair_name, pair_name);
    }
    
    #[tokio::test]
    async fn test_restore_from_backup() {
        let factory = FarmFactory::new(10.0);
        let chain_id = "test-chain";
        let pair_name = "TEST-USDT";
        
        // Tạo pool và đảm bảo backup
        let pool_id = factory.create_pool(chain_id, pair_name, None).await.unwrap();
        
        // Xóa manager hiện tại để mô phỏng việc khởi động lại
        {
            let mut managers = factory.managers.write().await;
            managers.clear();
        }
        
        // Khôi phục từ backup
        factory.restore_from_backup(chain_id).await.unwrap();
        
        // Kiểm tra xem pool có được khôi phục không
        let manager = factory.get_or_create_manager(chain_id).await.unwrap();
        let manager_lock = manager.read().await;
        
        let pools = manager_lock.get_all_pools().unwrap();
        assert_eq!(pools.len(), 1);
        assert_eq!(pools[0].pair_name, pair_name);
    }
}
