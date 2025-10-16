// P2P Pool Manager - Integrates pool mining with libp2p DHT
// This enables real peer-to-peer pool discovery and coordination

use crate::dht::DhtService;
use crate::pool::{MiningPool, ShareSubmission};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

const DHT_POOL_PREFIX: &str = "chiral:pool:";
const DHT_SHARE_PREFIX: &str = "chiral:share:";
const DHT_COORDINATOR_PREFIX: &str = "chiral:coordinator:";
const POOL_TTL_SECONDS: u64 = 3600; // 1 hour
const SHARE_TTL_SECONDS: u64 = 600;  // 10 minutes

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolAnnouncement {
    pub pool: MiningPool,
    pub coordinator_peer_id: Option<String>,
    pub announced_at: u64,
}

pub struct P2PPoolManager {
    dht: Arc<DhtService>,
    local_pools: Arc<RwLock<HashMap<String, MiningPool>>>,
    local_shares: Arc<RwLock<HashMap<String, Vec<ShareSubmission>>>>, // pool_id -> shares
    coordinator_map: Arc<RwLock<HashMap<String, String>>>, // pool_id -> coordinator_peer_id
    is_coordinator: Arc<RwLock<bool>>,
}

impl P2PPoolManager {
    pub fn new(dht: Arc<DhtService>) -> Self {
        Self {
            dht,
            local_pools: Arc::new(RwLock::new(HashMap::new())),
            local_shares: Arc::new(RwLock::new(HashMap::new())),
            coordinator_map: Arc::new(RwLock::new(HashMap::new())),
            is_coordinator: Arc::new(RwLock::new(false)),
        }
    }

    /// Announce a pool to the P2P network (currently using local storage)
    pub async fn announce_pool(&self, pool: MiningPool) -> Result<(), String> {
        info!("📡 Announcing pool to DHT: {} ({})", pool.name, pool.id);
        
        // Serialize pool to JSON
        let pool_json = serde_json::to_vec(&pool)
            .map_err(|e| format!("Failed to serialize pool: {}", e))?;
        
        // Store in DHT with key: "chiral:pool:{pool_id}"
        let key = format!("{}{}", DHT_POOL_PREFIX, pool.id);
        self.dht.put_record(&key, pool_json).await
            .map_err(|e| format!("Failed to announce pool to DHT: {}", e))?;
        
        // Also store locally for quick access
        let mut pools = self.local_pools.write().await;
        pools.insert(pool.id.clone(), pool.clone());
        
        info!("✅ Pool announced to DHT and stored locally");
        Ok(())
    }

    /// Discover pools from the P2P network (currently using local storage)
    /// TODO: Integrate with actual DHT once API is available
    pub async fn discover_pools(&self, pool_id: Option<&str>) -> Result<Vec<MiningPool>, String> {
        let mut discovered_pools = Vec::new();
        
        if let Some(id) = pool_id {
            info!("🔍 Querying DHT for specific pool: {}", id);
            
            // Try DHT first
            let key = format!("{}{}", DHT_POOL_PREFIX, id);
            match self.dht.get_record(&key).await {
                Ok(Some(data)) => {
                    match serde_json::from_slice::<MiningPool>(&data) {
                        Ok(pool) => {
                            info!("✅ Found pool {} in DHT", id);
                            discovered_pools.push(pool);
                        }
                        Err(e) => {
                            warn!("Failed to parse pool data from DHT: {}", e);
                        }
                    }
                }
                Ok(None) => {
                    info!("Pool {} not found in DHT, checking local storage", id);
                    // Fallback to local storage
                    let pools = self.local_pools.read().await;
                    if let Some(pool) = pools.get(id) {
                        discovered_pools.push(pool.clone());
                    }
                }
                Err(e) => {
                    warn!("DHT query error for pool {}: {}", id, e);
                    // Fallback to local storage
                    let pools = self.local_pools.read().await;
                    if let Some(pool) = pools.get(id) {
                        discovered_pools.push(pool.clone());
                    }
                }
            }
        } else {
            info!("📋 Returning all local pools");
            // For now, return all local pools
            // TODO: Implement pool directory in DHT for discovering all pools
            let pools = self.local_pools.read().await;
            discovered_pools.extend(pools.values().cloned());
        }
        
        Ok(discovered_pools)
    }

    /// Submit a share to the P2P network (currently using local storage)
    /// TODO: Integrate with actual DHT once API is available
    pub async fn submit_share(&self, share: ShareSubmission) -> Result<(), String> {
        info!("📤 Submitting share to P2P network for pool {}", share.pool_id);

        let mut shares = self.local_shares.write().await;
        shares.entry(share.pool_id.clone())
            .or_insert_with(Vec::new)
            .push(share);

        info!("✅ Share submitted to network");
        Ok(())
    }

    /// Get shares for a specific pool from the network
    pub async fn get_shares_for_pool(&self, pool_id: &str, since: Option<u64>) -> Result<Vec<ShareSubmission>, String> {
        info!("📥 Fetching shares for pool {} from network", pool_id);

        let shares = self.local_shares.read().await;
        let pool_shares = shares.get(pool_id).cloned().unwrap_or_default();
        drop(shares);

        // Filter by timestamp if provided
        let filtered: Vec<ShareSubmission> = if let Some(since_time) = since {
            pool_shares.into_iter().filter(|s| s.timestamp >= since_time).collect()
        } else {
            pool_shares
        };

        info!("✅ Retrieved {} shares for pool {}", filtered.len(), pool_id);
        Ok(filtered)
    }

    /// Become a coordinator for a pool
    pub async fn become_coordinator(&self, pool_id: &str) -> Result<bool, String> {
        info!("🎯 Attempting to become coordinator for pool {}", pool_id);

        let mut coordinators = self.coordinator_map.write().await;
        
        // Check if there's already a coordinator
        if coordinators.contains_key(pool_id) {
            info!("⚠️ Pool {} already has a coordinator", pool_id);
            return Ok(false);
        }

        // Become the coordinator
        coordinators.insert(pool_id.to_string(), "local".to_string());

        info!("✅ Now coordinating pool {}", pool_id);
        Ok(true)
    }

    /// Find the coordinator for a pool
    pub async fn find_coordinator(&self, pool_id: &str) -> Result<Option<String>, String> {
        let coordinators = self.coordinator_map.read().await;
        Ok(coordinators.get(pool_id).cloned())
    }

    /// List all pools we're tracking locally
    pub async fn list_local_pools(&self) -> Vec<MiningPool> {
        let pools = self.local_pools.read().await;
        pools.values().cloned().collect()
    }
}

// ============================================================================
// TAURI COMMANDS - Frontend-accessible P2P Pool Functions
// ============================================================================

use tokio::sync::Mutex as TokioMutex;

// Global P2P Pool Manager (initialized in main.rs)
lazy_static::lazy_static! {
    pub static ref P2P_POOL_MANAGER: Arc<TokioMutex<Option<P2PPoolManager>>> = 
        Arc::new(TokioMutex::new(None));
}

/// Initialize the P2P pool manager with DHT service
pub async fn init_p2p_pool_manager(dht: Arc<DhtService>) {
    let manager = P2PPoolManager::new(dht);
    let mut global_manager = P2P_POOL_MANAGER.lock().await;
    *global_manager = Some(manager);
    info!("P2P Pool Manager initialized");
}

#[tauri::command]
pub async fn p2p_announce_pool(pool: MiningPool) -> Result<(), String> {
    let manager_lock = P2P_POOL_MANAGER.lock().await;
    let manager = manager_lock.as_ref()
        .ok_or_else(|| "P2P Pool Manager not initialized".to_string())?;
    
    manager.announce_pool(pool).await
}

#[tauri::command]
pub async fn p2p_discover_pools(pool_id: Option<String>) -> Result<Vec<MiningPool>, String> {
    let manager_lock = P2P_POOL_MANAGER.lock().await;
    let manager = manager_lock.as_ref()
        .ok_or_else(|| "P2P Pool Manager not initialized".to_string())?;
    
    manager.discover_pools(pool_id.as_deref()).await
}

#[tauri::command]
pub async fn p2p_submit_share(share: ShareSubmission) -> Result<(), String> {
    let manager_lock = P2P_POOL_MANAGER.lock().await;
    let manager = manager_lock.as_ref()
        .ok_or_else(|| "P2P Pool Manager not initialized".to_string())?;
    
    manager.submit_share(share).await
}

#[tauri::command]
pub async fn p2p_get_shares_for_pool(pool_id: String, since: Option<u64>) -> Result<Vec<ShareSubmission>, String> {
    let manager_lock = P2P_POOL_MANAGER.lock().await;
    let manager = manager_lock.as_ref()
        .ok_or_else(|| "P2P Pool Manager not initialized".to_string())?;
    
    manager.get_shares_for_pool(&pool_id, since).await
}

#[tauri::command]
pub async fn p2p_become_coordinator(pool_id: String) -> Result<bool, String> {
    let manager_lock = P2P_POOL_MANAGER.lock().await;
    let manager = manager_lock.as_ref()
        .ok_or_else(|| "P2P Pool Manager not initialized".to_string())?;
    
    manager.become_coordinator(&pool_id).await
}

#[tauri::command]
pub async fn p2p_find_coordinator(pool_id: String) -> Result<Option<String>, String> {
    let manager_lock = P2P_POOL_MANAGER.lock().await;
    let manager = manager_lock.as_ref()
        .ok_or_else(|| "P2P Pool Manager not initialized".to_string())?;
    
    manager.find_coordinator(&pool_id).await
}

#[tauri::command]
pub async fn p2p_list_local_pools() -> Result<Vec<MiningPool>, String> {
    let manager_lock = P2P_POOL_MANAGER.lock().await;
    let manager = manager_lock.as_ref()
        .ok_or_else(|| "P2P Pool Manager not initialized".to_string())?;
    
    Ok(manager.list_local_pools().await)
}

#[tauri::command]
pub async fn p2p_is_coordinator() -> Result<bool, String> {
    // Simple implementation - just check if we're coordinating any pools
    let manager_lock = P2P_POOL_MANAGER.lock().await;
    let manager = manager_lock.as_ref()
        .ok_or_else(|| "P2P Pool Manager not initialized".to_string())?;
    
    let coordinators = manager.coordinator_map.read().await;
    Ok(!coordinators.is_empty())
}

#[tauri::command]
pub async fn p2p_resign_coordinator() -> Result<(), String> {
    let manager_lock = P2P_POOL_MANAGER.lock().await;
    let manager = manager_lock.as_ref()
        .ok_or_else(|| "P2P Pool Manager not initialized".to_string())?;
    
    let mut coordinators = manager.coordinator_map.write().await;
    coordinators.clear();
    info!("Resigned from all coordinator roles");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests would require a running DHT instance
    // For now, they serve as documentation of the API

    #[tokio::test]
    #[ignore] // Requires DHT setup
    async fn test_announce_and_discover() {
        // let dht = setup_test_dht().await;
        // let manager = P2PPoolManager::new(dht);
        
        // let pool = create_test_pool();
        // manager.announce_pool(pool).await.unwrap();
        
        // let discovered = manager.discover_pools(None).await.unwrap();
        // assert!(discovered.len() > 0);
    }
}
