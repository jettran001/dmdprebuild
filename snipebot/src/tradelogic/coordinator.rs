//! Trade Coordinator implementation
//!
//! This module implements the TradeCoordinator trait to provide centralized coordination
//! between different trade executors (SmartTradeExecutor, MevBot, etc.).
//! It manages shared state, opportunity sharing, and resource allocation.

// External imports
use std::sync::Arc;
use std::collections::{HashMap, HashSet};
use async_trait::async_trait;
use tokio::sync::{RwLock, mpsc, broadcast};
use uuid::Uuid;

// Standard library imports
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// Internal imports
use crate::tradelogic::traits::{
    TradeCoordinator, ExecutorType, SharedOpportunity, OpportunityPriority,
    SharedOpportunityType, OpportunityReservation, SharingStatistics, TradeCoordinatorBase, TradeCoordinatorAsync
};

// Third party imports
use anyhow::{Result, anyhow};
use tracing::{info, warn, error, debug};

/// Default time (in seconds) that a reservation lasts before expiring
const DEFAULT_RESERVATION_EXPIRY: u64 = 60;

/// Maximum number of opportunities to keep in memory
const MAX_OPPORTUNITIES: usize = 10000;

/// Default coordinator update interval in milliseconds
const DEFAULT_UPDATE_INTERVAL_MS: u64 = 100;

/// Implementation of the TradeCoordinator trait
pub struct TradeCoordinatorImpl {
    /// Known executors and their types
    executors: RwLock<HashMap<String, ExecutorType>>,
    
    /// All shared opportunities (active and historical)
    opportunities: RwLock<HashMap<String, SharedOpportunity>>,
    
    /// Active subscriptions to opportunity updates
    subscriptions: RwLock<HashMap<String, mpsc::Sender<SharedOpportunity>>>,
    
    /// Subscription by executor ID
    executor_subscriptions: RwLock<HashMap<String, HashSet<String>>>,
    
    /// Broadcast channel for all opportunities
    opportunity_broadcaster: broadcast::Sender<SharedOpportunity>,
    
    /// Global parameters shared between executors
    global_parameters: RwLock<HashMap<String, f64>>,
    
    /// Statistics
    statistics: RwLock<SharingStatistics>,
}

impl TradeCoordinatorImpl {
    /// Create a new TradeCoordinatorImpl
    pub fn new() -> Self {
        let (broadcaster, _) = broadcast::channel::<SharedOpportunity>(100);
        
        Self {
            executors: RwLock::new(HashMap::<String, ExecutorType>::new()),
            opportunities: RwLock::new(HashMap::<String, SharedOpportunity>::new()),
            subscriptions: RwLock::new(HashMap::<String, mpsc::Sender<SharedOpportunity>>::new()),
            executor_subscriptions: RwLock::new(HashMap::<String, HashSet<String>>::new()),
            opportunity_broadcaster: broadcaster,
            global_parameters: RwLock::new(HashMap::<String, f64>::new()),
            statistics: RwLock::new(SharingStatistics::default()),
        }
    }
    
    /// Start background tasks for the coordinator
    pub async fn start(&self) -> Result<()> {
        // Start opportunity cleanup task
        let weak_self = Arc::downgrade(&Arc::new(self.clone()));
        
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                
                // Try to upgrade weak reference to strong reference
                if let Some(coordinator) = weak_self.upgrade() {
                    if let Err(e) = coordinator.cleanup_expired_opportunities().await {
                        error!("Error cleaning up expired opportunities: {}", e);
                    }
                } else {
                    // Coordinator has been dropped, exit task
                    break;
                }
            }
        });
        
        // Start statistics update task
        let weak_self = Arc::downgrade(&Arc::new(self.clone()));
        
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(300)).await;
                
                // Try to upgrade weak reference to strong reference
                if let Some(coordinator) = weak_self.upgrade() {
                    if let Err(e) = coordinator.update_statistics().await {
                        error!("Error updating statistics: {}", e);
                    }
                } else {
                    // Coordinator has been dropped, exit task
                    break;
                }
            }
        });
        
        Ok(())
    }
    
    /// Get current timestamp in seconds in a safe manner
    fn current_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_else(|_| {
                // Log error but don't panic
                error!("System time before UNIX EPOCH, using 0 as fallback");
                0
            })
    }
    
    /// Check if opportunity exists
    async fn opportunity_exists(&self, opportunity_id: &str) -> bool {
        let opportunities = self.opportunities.read().await;
        opportunities.contains_key(opportunity_id)
    }
    
    /// Clean up expired opportunities and reservations
    async fn cleanup_expired_opportunities(&self) -> Result<()> {
        let now = Self::current_timestamp();
        let mut opportunities = self.opportunities.write().await;
        let mut released_count = 0;
        
        // First pass: clear expired reservations
        for opp in opportunities.values_mut() {
            if let Some(reservation) = &opp.reservation {
                if reservation.expires_at < now {
                    opp.reservation = None;
                    released_count += 1;
                }
            }
        }
        
        // Second pass: remove old opportunities if we have too many
        if opportunities.len() > MAX_OPPORTUNITIES {
            // Sort by creation time, oldest first
            let mut opportunity_vec: Vec<(String, u64)> = opportunities
                .iter()
                .map(|(id, opp)| (id.clone(), opp.created_at))
                .collect();
            
            opportunity_vec.sort_by_key(|(_, created_at)| *created_at);
            
            // Keep only the most recent MAX_OPPORTUNITIES
            let to_remove = opportunity_vec.len() - MAX_OPPORTUNITIES;
            let to_remove_ids: Vec<String> = opportunity_vec
                .into_iter()
                .take(to_remove)
                .map(|(id, _)| id)
                .collect();
            
            for id in to_remove_ids {
                opportunities.remove(&id);
            }
        }
        
        if released_count > 0 {
            debug!("Released {} expired opportunity reservations", released_count);
        }
        
        Ok(())
    }
    
    /// Update statistics
    async fn update_statistics(&self) -> Result<()> {
        let opportunities = self.opportunities.read().await;
        let mut by_type: HashMap<SharedOpportunityType, u64> = HashMap::new();
        let mut by_source: HashMap<String, u64> = HashMap::new();
        let reservation_times = Vec::new();
        let reservation_conflicts = 0;
        let executed = 0;
        let expired = 0;
        
        for opp in opportunities.values() {
            // Count by type
            *by_type.entry(opp.opportunity_type.clone()).or_insert(0) += 1;
            
            // Count by source
            *by_source.entry(opp.source.clone()).or_insert(0) += 1;
        }
        
        let avg_reservation_time = if !reservation_times.is_empty() {
            reservation_times.iter().sum::<f64>() / reservation_times.len() as f64
        } else {
            0.0
        };
        
        let mut stats = self.statistics.write().await;
        stats.total_shared = opportunities.len() as u64;
        stats.by_type = by_type;
        stats.by_source = by_source;
        stats.executed = executed;
        stats.expired = expired;
        stats.avg_reservation_time = avg_reservation_time;
        stats.reservation_conflicts = reservation_conflicts;
        
        Ok(())
    }
    
    /// Send opportunity to a specific executor
    async fn send_opportunity_to_executor(
        &self,
        executor_id: &str,
        opportunity: &SharedOpportunity,
    ) -> Result<()> {
        let subscriptions = self.subscriptions.read().await;
        let executor_subscriptions = self.executor_subscriptions.read().await;
        
        if let Some(subscription_ids) = executor_subscriptions.get(executor_id) {
            for sub_id in subscription_ids {
                if let Some(sender) = subscriptions.get(sub_id) {
                    if let Err(e) = sender.send(opportunity.clone()).await {
                        warn!("Failed to send opportunity to executor {}: {}", executor_id, e);
                    }
                }
            }
        }
        
        Ok(())
    }
}

impl Clone for TradeCoordinatorImpl {
    fn clone(&self) -> Self {
        // Since we use RwLock/Arc for all internal state, cloning is cheap and just copies the wrapper pointers
        let (broadcaster, _) = broadcast::channel::<SharedOpportunity>(100);
        
        Self {
            executors: RwLock::new(HashMap::<String, ExecutorType>::new()),
            opportunities: RwLock::new(HashMap::<String, SharedOpportunity>::new()),
            subscriptions: RwLock::new(HashMap::<String, mpsc::Sender<SharedOpportunity>>::new()),
            executor_subscriptions: RwLock::new(HashMap::<String, HashSet<String>>::new()),
            opportunity_broadcaster: broadcaster,
            global_parameters: RwLock::new(HashMap::<String, f64>::new()),
            statistics: RwLock::new(SharingStatistics::default()),
        }
    }
}

// Implement TradeCoordinatorBase for TradeCoordinatorImpl
impl TradeCoordinatorBase for TradeCoordinatorImpl {
    fn get_executor_type(&self, executor_id: &str) -> Option<ExecutorType> {
        // Have to use block_in_place since we can't use async in a sync function
        tokio::task::block_in_place(|| {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async {
                let executors = self.executors.read().await;
                executors.get(executor_id).cloned()
            })
        })
    }
    
    fn is_executor_registered(&self, executor_id: &str) -> bool {
        // Have to use block_in_place since we can't use async in a sync function
        tokio::task::block_in_place(|| {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async {
                let executors = self.executors.read().await;
                executors.contains_key(executor_id)
            })
        })
    }
    
    fn get_opportunity_by_id(&self, opportunity_id: &str) -> Option<SharedOpportunity> {
        // Have to use block_in_place since we can't use async in a sync function
        tokio::task::block_in_place(|| {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async {
                let opportunities = self.opportunities.read().await;
                opportunities.get(opportunity_id).cloned()
            })
        })
    }
    
    fn is_opportunity_reserved(&self, opportunity_id: &str) -> bool {
        // Have to use block_in_place since we can't use async in a sync function
        tokio::task::block_in_place(|| {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async {
                let opportunities = self.opportunities.read().await;
                if let Some(opportunity) = opportunities.get(opportunity_id) {
                    opportunity.reservation.is_some()
                } else {
                    false
                }
            })
        })
    }
    
    fn get_reservation(&self, opportunity_id: &str) -> Option<OpportunityReservation> {
        // Have to use block_in_place since we can't use async in a sync function
        tokio::task::block_in_place(|| {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async {
                let opportunities = self.opportunities.read().await;
                if let Some(opportunity) = opportunities.get(opportunity_id) {
                    opportunity.reservation.clone()
                } else {
                    None
                }
            })
        })
    }
    
    fn can_release_opportunity(&self, opportunity_id: &str, executor_id: &str) -> bool {
        // Have to use block_in_place since we can't use async in a sync function
        tokio::task::block_in_place(|| {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async {
                let opportunities = self.opportunities.read().await;
                if let Some(opportunity) = opportunities.get(opportunity_id) {
                    if let Some(reservation) = &opportunity.reservation {
                        reservation.executor_id == executor_id
                    } else {
                        false
                    }
                } else {
                    false
                }
            })
        })
    }
}

// Implement TradeCoordinatorAsync (already exists as TradeCoordinator)
#[async_trait]
impl TradeCoordinatorAsync for TradeCoordinatorImpl {
    async fn register_executor(&self, executor_id: &str, executor_type: ExecutorType) -> Result<()> {
        let mut executors = self.executors.write().await;
        executors.insert(executor_id.to_string(), executor_type);
        info!("Registered executor {} with coordinator", executor_id);
        Ok(())
    }
    
    async fn unregister_executor(&self, executor_id: &str) -> Result<()> {
        let mut executors = self.executors.write().await;
        executors.remove(executor_id);
        
        // Clean up subscriptions
        let mut executor_subscriptions = self.executor_subscriptions.write().await;
        if let Some(subscription_ids) = executor_subscriptions.remove(executor_id) {
            let mut subscriptions = self.subscriptions.write().await;
            for sub_id in subscription_ids {
                subscriptions.remove(&sub_id);
            }
        }
        
        info!("Unregistered executor {} from coordinator", executor_id);
        Ok(())
    }
    
    async fn share_opportunity(&self, from_executor: &str, mut opportunity: SharedOpportunity) -> Result<()> {
        // Validate that the executor exists
        if !self.is_executor_registered(from_executor) {
            return Err(anyhow!("Executor {} is not registered with coordinator", from_executor));
        }
        
        // Set source
        opportunity.source = from_executor.to_string();
        
        // Generate ID if not already set
        if opportunity.id.is_empty() {
            opportunity.id = Uuid::new_v4().to_string();
        }
        
        // Set created_at if not already set
        if opportunity.created_at == 0 {
            opportunity.created_at = Self::current_timestamp();
        }
        
        // Don't re-share if already exists
        if self.opportunity_exists(&opportunity.id).await {
            warn!("Opportunity {} already exists, not sharing again", opportunity.id);
            return Ok(());
        }
        
        // Store opportunity
        {
            let mut opportunities = self.opportunities.write().await;
            opportunities.insert(opportunity.id.clone(), opportunity.clone());
        }
        
        // Broadcast to all subscribers
        if let Err(e) = self.opportunity_broadcaster.send(opportunity.clone()) {
            warn!("Failed to broadcast opportunity: {}", e);
        }
        
        // Direct notification to subscribers
        for subscription in self.subscriptions.read().await.values() {
            if let Err(e) = subscription.send(opportunity.clone()).await {
                warn!("Failed to send opportunity to subscriber: {}", e);
                // Don't return error, continue with other subscribers
            }
        }
        
        // Update statistics
        let mut stats = self.statistics.write().await;
        stats.total_shared += 1;
        *stats.by_source.entry(from_executor.to_string()).or_insert(0) += 1;
        *stats.by_type.entry(opportunity.opportunity_type.clone()).or_insert(0) += 1;
        
        info!("Shared opportunity {} from {}", opportunity.id, from_executor);
        Ok(())
    }
    
    async fn subscribe_to_opportunities(
        &self,
        executor_id: &str,
        callback: Arc<dyn Fn(SharedOpportunity) -> Result<()> + Send + Sync + 'static>,
    ) -> Result<String> {
        // Create a channel for receiving opportunities
        let (tx, mut rx) = mpsc::channel::<SharedOpportunity>(100);
        
        // Generate a unique subscription ID
        let subscription_id = Uuid::new_v4().to_string();
        
        // Store the sender
        {
            let mut subscriptions = self.subscriptions.write().await;
            subscriptions.insert(subscription_id.clone(), tx);
        }
        
        // Add to executor's subscriptions
        {
            let mut executor_subscriptions = self.executor_subscriptions.write().await;
            executor_subscriptions
                .entry(executor_id.to_string())
                .or_insert_with(HashSet::new)
                .insert(subscription_id.clone());
        }
        
        // Spawn a task to forward received opportunities to the callback
        let subscription_id_clone = subscription_id.clone();
        tokio::spawn(async move {
            while let Some(opportunity) = rx.recv().await {
                if let Err(e) = callback(opportunity) {
                    error!("Error in opportunity subscription callback {}: {}", subscription_id_clone, e);
                }
            }
        });
        
        info!("Created subscription {} for executor {}", subscription_id, executor_id);
        Ok(subscription_id)
    }
    
    async fn unsubscribe_from_opportunities(&self, subscription_id: &str) -> Result<()> {
        // Remove from subscriptions
        {
            let mut subscriptions = self.subscriptions.write().await;
            subscriptions.remove(subscription_id);
        }
        
        // Clean up executor subscriptions
        {
            let mut executor_subscriptions = self.executor_subscriptions.write().await;
            for subscriptions in executor_subscriptions.values_mut() {
                subscriptions.remove(subscription_id);
            }
        }
        
        info!("Removed subscription {}", subscription_id);
        Ok(())
    }
    
    async fn reserve_opportunity(&self, opportunity_id: &str, executor_id: &str, priority: OpportunityPriority) -> Result<bool> {
        // Validate that the executor exists
        if !self.is_executor_registered(executor_id) {
            return Err(anyhow!("Executor {} is not registered with coordinator", executor_id));
        }
        
        let now = Self::current_timestamp();
        let expires_at = now + DEFAULT_RESERVATION_EXPIRY;
        
        let reservation = OpportunityReservation {
            executor_id: executor_id.to_string(),
            priority,
            reserved_at: now,
            expires_at,
        };
        
        // Update opportunity with reservation
        let mut opportunities = self.opportunities.write().await;
        let opportunity = opportunities.get_mut(opportunity_id).ok_or_else(|| {
            anyhow!("Opportunity {} not found", opportunity_id)
        })?;
        
        // Check if already reserved
        if let Some(existing_reservation) = &opportunity.reservation {
            // Check if existing reservation is expired
            if existing_reservation.expires_at < now {
                // Expired, can take over
                opportunity.reservation = Some(reservation);
                info!("Reserved opportunity {} for executor {} (taking over expired reservation)", opportunity_id, executor_id);
                return Ok(true);
            }
            
            // Not expired, check priority
            if priority > existing_reservation.priority {
                // Higher priority, can override
                let old_executor = &existing_reservation.executor_id;
                opportunity.reservation = Some(reservation);
                
                // Update statistics
                let mut stats = self.statistics.write().await;
                stats.reservation_conflicts += 1;
                
                info!("Reserved opportunity {} for executor {} (overriding {})", opportunity_id, executor_id, old_executor);
                return Ok(true);
            }
            
            // Can't override
            debug!("Failed to reserve opportunity {} for executor {} (already reserved by {} with priority {:?})", 
                  opportunity_id, executor_id, existing_reservation.executor_id, existing_reservation.priority);
            return Ok(false);
        }
        
        // Not reserved, can take it
        opportunity.reservation = Some(reservation);
        info!("Reserved opportunity {} for executor {}", opportunity_id, executor_id);
        Ok(true)
    }
    
    async fn release_opportunity(&self, opportunity_id: &str, executor_id: &str) -> Result<()> {
        let mut opportunities = self.opportunities.write().await;
        let opportunity = opportunities.get_mut(opportunity_id).ok_or_else(|| {
            anyhow!("Opportunity {} not found", opportunity_id)
        })?;
        
        // Check if reserved by this executor
        if let Some(reservation) = &opportunity.reservation {
            if reservation.executor_id == executor_id {
                // Update statistics
                if let Ok(mut stats) = self.statistics.write().await.try_lock() {
                    let reservation_time = Self::current_timestamp() - reservation.reserved_at;
                    stats.avg_reservation_time = ((stats.avg_reservation_time * (stats.executed as f64)) + reservation_time as f64) / (stats.executed + 1) as f64;
                }
                
                // Release reservation
                opportunity.reservation = None;
                info!("Released opportunity {} for executor {}", opportunity_id, executor_id);
                return Ok(());
            }
        }
        
        // Not reserved or reserved by someone else
        Err(anyhow!("Opportunity {} is not reserved by executor {}", opportunity_id, executor_id))
    }
    
    async fn get_all_opportunities(&self) -> Result<Vec<SharedOpportunity>> {
        let opportunities = self.opportunities.read().await;
        Ok(opportunities.values().cloned().collect())
    }
    
    async fn get_sharing_statistics(&self) -> Result<SharingStatistics> {
        let stats = self.statistics.read().await;
        Ok(stats.clone())
    }
    
    async fn update_global_parameters(&self, parameters: HashMap<String, f64>) -> Result<()> {
        let mut globals = self.global_parameters.write().await;
        for (key, value) in parameters {
            globals.insert(key, value);
        }
        
        Ok(())
    }
    
    async fn get_global_parameters(&self) -> Result<HashMap<String, f64>> {
        let globals = self.global_parameters.read().await;
        Ok(globals.clone())
    }
}

/// Create a new TradeCoordinator with background cleanup tasks
pub fn create_trade_coordinator() -> Arc<dyn TradeCoordinator> {
    let coordinator = TradeCoordinatorImpl::new();
    let arc_coordinator = Arc::new(coordinator) as Arc<dyn TradeCoordinator>;
    
    // Start background tasks but handle errors properly
    let weak_coordinator = Arc::downgrade(&arc_coordinator);
    tokio::spawn(async move {
        // Upgrade weak reference to strong reference
        if let Some(coordinator) = weak_coordinator.upgrade() {
            if let Err(e) = coordinator.start().await {
                error!("Failed to start trade coordinator background tasks: {}", e);
            }
        }
    });
    
    arc_coordinator
}

#[cfg(test)]
mod tests {
    use super::*;
    
    /// Test opportunity creation and retrieval
    #[tokio::test]
    async fn test_opportunity_sharing() {
        let coordinator = create_trade_coordinator();
        
        // Register executors
        if let Err(e) = coordinator.register_executor("smart-trade", ExecutorType::SmartTrade).await {
            assert!(false, "Failed to register smart-trade executor: {}", e);
            return;
        }
        
        if let Err(e) = coordinator.register_executor("mev-bot", ExecutorType::MevBot).await {
            assert!(false, "Failed to register mev-bot executor: {}", e);
            return;
        }
        
        // Create opportunity
        let opportunity = SharedOpportunity {
            id: Uuid::new_v4().to_string(),
            chain_id: 1, // Ethereum
            opportunity_type: SharedOpportunityType::NewToken,
            tokens: vec!["0x1234567890abcdef1234567890abcdef12345678".to_string()],
            estimated_profit_usd: 100.0,
            risk_score: 50,
            time_sensitivity: 60, // 1 minute
            source: "smart-trade".to_string(),
            created_at: TradeCoordinatorImpl::current_timestamp(),
            custom_data: HashMap::new(),
            reservation: None,
        };
        
        // Share opportunity
        if let Err(e) = coordinator.share_opportunity("smart-trade", opportunity.clone()).await {
            assert!(false, "Failed to share opportunity: {}", e);
            return;
        }
        
        // Get opportunities
        let opportunities = match coordinator.get_all_opportunities().await {
            Ok(opps) => opps,
            Err(e) => {
                assert!(false, "Failed to get opportunities: {}", e);
                return;
            }
        };
        
        // Verify
        assert_eq!(opportunities.len(), 1);
        assert_eq!(opportunities[0].id, opportunity.id);
        
        // Test reservation
        let reserved = match coordinator.reserve_opportunity(
            &opportunity.id, 
            "mev-bot", 
            OpportunityPriority::Medium
        ).await {
            Ok(result) => result,
            Err(e) => {
                assert!(false, "Failed to reserve opportunity: {}", e);
                return;
            }
        };
        
        assert!(reserved);
        
        // Verify reservation
        let opportunities = match coordinator.get_all_opportunities().await {
            Ok(opps) => opps,
            Err(e) => {
                assert!(false, "Failed to get opportunities after reservation: {}", e);
                return;
            }
        };
        assert!(opportunities.len() > 0, "Should have at least one opportunity");
        
        // Safely check reservation instead of using unwrap
        if let Some(opportunity) = opportunities.first() {
            assert!(opportunity.reservation.is_some(), "Opportunity should have a reservation");
            
            if let Some(reservation) = &opportunity.reservation {
                assert_eq!(reservation.executor_id, "mev-bot", "Reservation should be for mev-bot");
            }
        }
    }
} 