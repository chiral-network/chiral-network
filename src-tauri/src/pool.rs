// Decentralized Mining Pool System
// Implements pool discovery and management for distributed mining

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{command, AppHandle, Manager};
use tokio::sync::Mutex;
use tracing::{error, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiningPool {
    pub id: String,
    pub name: String,
    pub url: String,
    pub description: String,
    pub fee_percentage: f64,
    pub miners_count: u32,
    pub total_hashrate: String,
    pub last_block_time: u64,
    pub blocks_found_24h: u32,
    pub region: String,
    pub status: PoolStatus,
    pub min_payout: f64,
    pub payment_method: String,
    pub created_by: Option<String>, // Address of pool creator
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolStats {
    pub connected_miners: u32,
    pub pool_hashrate: String,
    pub your_hashrate: String,
    pub your_share_percentage: f64,
    pub shares_submitted: u32,
    pub shares_accepted: u32,
    pub estimated_payout_24h: f64,
    pub last_share_time: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PoolStatus {
    Active,
    Maintenance,
    Full,
    Offline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinedPoolInfo {
    pub pool: MiningPool,
    pub stats: PoolStats,
    pub joined_at: u64,
}

// Global state for mining pool management
lazy_static::lazy_static! {
    static ref AVAILABLE_POOLS: Arc<Mutex<Vec<MiningPool>>> = Arc::new(Mutex::new(create_default_pools()));
    static ref CURRENT_POOL: Arc<Mutex<Option<JoinedPoolInfo>>> = Arc::new(Mutex::new(None));
    static ref USER_CREATED_POOLS: Arc<Mutex<Vec<MiningPool>>> = Arc::new(Mutex::new(Vec::new()));
}

fn create_default_pools() -> Vec<MiningPool> {
    vec![
        MiningPool {
            id: "chiral-main".to_string(),
            name: "Chiral Main Pool".to_string(),
            url: "stratum+tcp://main.chiral.network:3333".to_string(),
            description: "Official Chiral Network mining pool with 0% fees".to_string(),
            fee_percentage: 0.0,
            miners_count: 156,
            total_hashrate: "2.4 GH/s".to_string(),
            last_block_time: get_current_timestamp() - 180, // 3 minutes ago
            blocks_found_24h: 24,
            region: "Global".to_string(),
            status: PoolStatus::Active,
            min_payout: 1.0,
            payment_method: "PPLNS".to_string(),
            created_by: None,
        },
        MiningPool {
            id: "community-asia".to_string(),
            name: "Asia Community Pool".to_string(),
            url: "stratum+tcp://asia.chiral.community:4444".to_string(),
            description: "Low-latency pool for Asian miners with regional nodes".to_string(),
            fee_percentage: 1.0,
            miners_count: 89,
            total_hashrate: "1.2 GH/s".to_string(),
            last_block_time: get_current_timestamp() - 420, // 7 minutes ago
            blocks_found_24h: 18,
            region: "Asia".to_string(),
            status: PoolStatus::Active,
            min_payout: 0.5,
            payment_method: "PPS".to_string(),
            created_by: None,
        },
        MiningPool {
            id: "europe-stable".to_string(),
            name: "Europe Stable Mining".to_string(),
            url: "stratum+tcp://eu.stable-mining.org:3334".to_string(),
            description: "Stable EU-based pool with consistent payouts".to_string(),
            fee_percentage: 1.5,
            miners_count: 234,
            total_hashrate: "3.8 GH/s".to_string(),
            last_block_time: get_current_timestamp() - 95, // ~1.5 minutes ago
            blocks_found_24h: 32,
            region: "Europe".to_string(),
            status: PoolStatus::Active,
            min_payout: 2.0,
            payment_method: "PPLNS".to_string(),
            created_by: None,
        },
        MiningPool {
            id: "small-miners".to_string(),
            name: "Small Miners United".to_string(),
            url: "stratum+tcp://small.miners.net:3335".to_string(),
            description: "Dedicated pool for small-scale miners with low minimum payout"
                .to_string(),
            fee_percentage: 0.5,
            miners_count: 67,
            total_hashrate: "845 MH/s".to_string(),
            last_block_time: get_current_timestamp() - 1200, // 20 minutes ago
            blocks_found_24h: 12,
            region: "Americas".to_string(),
            status: PoolStatus::Active,
            min_payout: 0.1,
            payment_method: "PPS+".to_string(),
            created_by: None,
        },
        MiningPool {
            id: "experimental-pool".to_string(),
            name: "Experimental Features Pool".to_string(),
            url: "stratum+tcp://experimental.chiral.dev:3336".to_string(),
            description: "Testing new pool features and optimizations".to_string(),
            fee_percentage: 2.0,
            miners_count: 23,
            total_hashrate: "387 MH/s".to_string(),
            last_block_time: get_current_timestamp() - 2400, // 40 minutes ago
            blocks_found_24h: 8,
            region: "Global".to_string(),
            status: PoolStatus::Maintenance,
            min_payout: 0.25,
            payment_method: "PROP".to_string(),
            created_by: None,
        },
    ]
}

fn get_current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(std::time::Duration::from_secs(0))
        .as_secs()
}

/// Get path to user pools storage file
fn get_user_pools_path(app_handle: &AppHandle) -> Result<PathBuf, String> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data directory: {}", e))?;

    std::fs::create_dir_all(&app_data_dir)
        .map_err(|e| format!("Failed to create app data directory: {}", e))?;

    Ok(app_data_dir.join("user_pools.json"))
}

/// Load user-created pools from persistent storage
async fn load_user_pools(app_handle: &AppHandle) -> Result<Vec<MiningPool>, String> {
    let pools_path = get_user_pools_path(app_handle)?;

    if !pools_path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&pools_path)
        .map_err(|e| format!("Failed to read user pools: {}", e))?;

    let pools: Vec<MiningPool> = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse user pools: {}", e))?;

    info!("Loaded {} user-created pools from storage", pools.len());
    Ok(pools)
}

/// Save user-created pools to persistent storage
async fn save_user_pools(app_handle: &AppHandle, pools: &[MiningPool]) -> Result<(), String> {
    let pools_path = get_user_pools_path(app_handle)?;

    let content = serde_json::to_string_pretty(pools)
        .map_err(|e| format!("Failed to serialize user pools: {}", e))?;

    std::fs::write(&pools_path, content)
        .map_err(|e| format!("Failed to write user pools: {}", e))?;

    info!("Saved {} user-created pools to storage", pools.len());
    Ok(())
}

/// Validate pool URL format
fn validate_pool_url(url: &str) -> Result<(String, u16), String> {
    // Check if URL starts with stratum protocol
    if !url.starts_with("stratum+tcp://") && !url.starts_with("stratum://") {
        return Err("Pool URL must use stratum+tcp:// or stratum:// protocol".to_string());
    }

    // Extract host and port
    let url_without_protocol = url
        .strip_prefix("stratum+tcp://")
        .or_else(|| url.strip_prefix("stratum://"))
        .ok_or("Invalid pool URL format".to_string())?;

    let parts: Vec<&str> = url_without_protocol.split(':').collect();
    if parts.len() != 2 {
        return Err("Pool URL must include host:port (e.g., stratum+tcp://pool.example.com:3333)".to_string());
    }

    let host = parts[0].to_string();
    let port = parts[1]
        .parse::<u16>()
        .map_err(|_| "Invalid port number".to_string())?;

    if host.is_empty() {
        return Err("Pool host cannot be empty".to_string());
    }

    if port == 0 {
        return Err("Pool port must be greater than 0".to_string());
    }

    Ok((host, port))
}

/// Check if pool URL is reachable
async fn check_pool_connectivity(host: &str, port: u16) -> bool {
    use tokio::net::TcpStream;
    use tokio::time::{timeout, Duration};

    let address = format!("{}:{}", host, port);
    match timeout(Duration::from_secs(5), TcpStream::connect(&address)).await {
        Ok(Ok(_)) => {
            info!("Pool {}:{} is reachable", host, port);
            true
        }
        Ok(Err(e)) => {
            info!("Pool {}:{} connection failed: {}", host, port, e);
            false
        }
        Err(_) => {
            info!("Pool {}:{} connection timed out", host, port);
            false
        }
    }
}

#[command]
pub async fn discover_mining_pools(app_handle: AppHandle) -> Result<Vec<MiningPool>, String> {
    info!("Discovering available mining pools");

    let pools = AVAILABLE_POOLS.lock().await;
    let mut all_pools = pools.clone();

    // Load user-created pools from persistent storage
    match load_user_pools(&app_handle).await {
        Ok(loaded_pools) => {
            let mut user_pools = USER_CREATED_POOLS.lock().await;
            *user_pools = loaded_pools.clone();
            all_pools.extend(loaded_pools);
        }
        Err(e) => {
            error!("Failed to load user pools from storage: {}", e);
            // Fallback to in-memory pools
            let user_pools = USER_CREATED_POOLS.lock().await;
            all_pools.extend(user_pools.clone());
        }
    }

    info!("Found {} mining pools", all_pools.len());
    Ok(all_pools)
}

#[command]
pub async fn create_mining_pool(
    app_handle: AppHandle,
    address: String,
    name: String,
    description: String,
    fee_percentage: f64,
    min_payout: f64,
    payment_method: String,
    region: String,
) -> Result<MiningPool, String> {
    info!(
        "Creating new mining pool: {} by {}",
        name, address
    );

    if name.trim().is_empty() {
        return Err("Pool name cannot be empty".to_string());
    }

    if fee_percentage < 0.0 || fee_percentage > 10.0 {
        return Err("Fee percentage must be between 0% and 10%".to_string());
    }

    let pool_id = format!(
        "user-{}-{}",
        address[..8].to_string(),
        get_current_timestamp()
    );
    let new_pool = MiningPool {
        id: pool_id.clone(),
        name: name.clone(),
        url: format!("stratum+tcp://{}:3333", pool_id),
        description,
        fee_percentage,
        miners_count: 1,
        total_hashrate: "0 H/s".to_string(),
        last_block_time: 0,
        blocks_found_24h: 0,
        region,
        status: PoolStatus::Active,
        min_payout,
        payment_method,
        created_by: Some(address.clone()),
    };

    // Add to in-memory pools
    let mut user_pools = USER_CREATED_POOLS.lock().await;
    user_pools.push(new_pool.clone());

    // Persist to storage
    if let Err(e) = save_user_pools(&app_handle, &user_pools).await {
        error!("Failed to save user pools: {}", e);
        // Continue even if save fails
    }

    info!("Successfully created pool: {}", name);
    Ok(new_pool)
}

#[command]
pub async fn join_mining_pool(pool_id: String, address: String) -> Result<JoinedPoolInfo, String> {
    info!(
        "Attempting to join mining pool: {} with address: {}",
        pool_id, address
    );

    // Check if already in a pool
    let current_pool = CURRENT_POOL.lock().await;
    if current_pool.is_some() {
        return Err("Already connected to a mining pool. Leave current pool first.".to_string());
    }
    drop(current_pool);

    // Find the pool
    let pools = AVAILABLE_POOLS.lock().await;
    let user_pools = USER_CREATED_POOLS.lock().await;

    let pool = pools
        .iter()
        .chain(user_pools.iter())
        .find(|p| p.id == pool_id)
        .cloned()
        .ok_or_else(|| "Pool not found".to_string())?;

    if matches!(pool.status, PoolStatus::Offline) {
        return Err("Pool is currently offline".to_string());
    }

    // Validate pool URL format
    let (host, port) = validate_pool_url(&pool.url)?;

    // Check pool connectivity
    info!("Checking connectivity to {}:{}", host, port);
    let is_reachable = check_pool_connectivity(&host, port).await;
    if !is_reachable {
        return Err(format!("Unable to connect to pool at {}:{}", host, port));
    }

    let stats = PoolStats {
        connected_miners: pool.miners_count + 1,
        pool_hashrate: pool.total_hashrate.clone(),
        your_hashrate: "0 H/s".to_string(),
        your_share_percentage: 0.0,
        shares_submitted: 0,
        shares_accepted: 0,
        estimated_payout_24h: 0.0,
        last_share_time: get_current_timestamp(),
    };

    let joined_info = JoinedPoolInfo {
        pool: pool.clone(),
        stats,
        joined_at: get_current_timestamp(),
    };

    // Update current pool
    let mut current = CURRENT_POOL.lock().await;
    *current = Some(joined_info.clone());

    info!("Successfully joined pool: {}", pool.name);
    Ok(joined_info)
}

#[command]
pub async fn leave_mining_pool() -> Result<(), String> {
    info!("Leaving current mining pool");

    let mut current_pool = CURRENT_POOL.lock().await;
    if current_pool.is_none() {
        return Err("Not currently connected to any pool".to_string());
    }

    let pool_name = current_pool.as_ref()
        .map(|p| p.pool.name.clone())
        .unwrap_or_else(|| "Unknown Pool".to_string());
    *current_pool = None;

    // Simulate disconnection delay
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    info!("Successfully left pool: {}", pool_name);
    Ok(())
}

#[command]
pub async fn get_current_pool_info() -> Result<Option<JoinedPoolInfo>, String> {
    let current_pool = CURRENT_POOL.lock().await;
    Ok(current_pool.clone())
}

#[command]
pub async fn get_pool_stats() -> Result<Option<PoolStats>, String> {
    let mut current_pool = CURRENT_POOL.lock().await;

    if let Some(ref mut pool_info) = *current_pool {
        let time_mining = get_current_timestamp() - pool_info.joined_at;

        // Update stats based on actual time connected
        pool_info.stats.connected_miners = pool_info.pool.miners_count;
        pool_info.stats.pool_hashrate = pool_info.pool.total_hashrate.clone();

        // Calculate shares based on time mining (1 share per 30 seconds)
        let expected_shares = (time_mining / 30) as u32;
        if expected_shares > pool_info.stats.shares_submitted {
            pool_info.stats.shares_submitted = expected_shares;
            // 95% acceptance rate
            pool_info.stats.shares_accepted = (expected_shares as f32 * 0.95) as u32;
            pool_info.stats.last_share_time = get_current_timestamp();
        }

        // Calculate hashrate based on shares submitted
        if time_mining > 0 {
            let shares_per_second = pool_info.stats.shares_submitted as f64 / time_mining as f64;
            let hashrate_khs = shares_per_second * 1000.0; // Convert to KH/s
            pool_info.stats.your_hashrate = format!("{:.1} KH/s", hashrate_khs);

            // Calculate share percentage of pool
            if let Ok(pool_hashrate_str) = extract_hashrate_number(&pool_info.pool.total_hashrate) {
                let your_hashrate = hashrate_khs;
                let pool_hashrate = pool_hashrate_str * 1_000_000.0; // Pool is in MH/s or GH/s
                pool_info.stats.your_share_percentage = (your_hashrate / pool_hashrate) * 100.0;

                // Estimate 24h payout based on share percentage and pool blocks
                let daily_blocks = pool_info.pool.blocks_found_24h as f64;
                let block_reward = 2.0; // Chiral block reward
                let expected_reward = (pool_info.stats.your_share_percentage / 100.0) * daily_blocks * block_reward;
                pool_info.stats.estimated_payout_24h = expected_reward;
            }
        }

        Ok(Some(pool_info.stats.clone()))
    } else {
        Ok(None)
    }
}

/// Extract numeric hashrate value from string (e.g., "2.4 GH/s" -> 2400.0 MH/s)
fn extract_hashrate_number(hashrate_str: &str) -> Result<f64, String> {
    let parts: Vec<&str> = hashrate_str.split_whitespace().collect();
    if parts.len() < 2 {
        return Err("Invalid hashrate format".to_string());
    }

    let number: f64 = parts[0]
        .parse()
        .map_err(|_| "Failed to parse hashrate number".to_string())?;

    // Convert to MH/s base
    let multiplier = match parts[1] {
        "H/s" => 0.000001,
        "KH/s" => 0.001,
        "MH/s" => 1.0,
        "GH/s" => 1000.0,
        "TH/s" => 1_000_000.0,
        _ => return Err("Unknown hashrate unit".to_string()),
    };

    Ok(number * multiplier)
}

#[command]
pub async fn update_pool_discovery() -> Result<(), String> {
    info!("Updating pool health status");

    let mut pools = AVAILABLE_POOLS.lock().await;

    // Check health of each pool
    for pool in pools.iter_mut() {
        // Validate URL and check connectivity
        if let Ok((host, port)) = validate_pool_url(&pool.url) {
            let is_reachable = check_pool_connectivity(&host, port).await;

            // Update pool status based on connectivity
            if is_reachable {
                if matches!(pool.status, PoolStatus::Offline) {
                    pool.status = PoolStatus::Active;
                    info!("Pool {} is now online", pool.name);
                }
            } else {
                if !matches!(pool.status, PoolStatus::Offline) {
                    pool.status = PoolStatus::Offline;
                    info!("Pool {} is now offline", pool.name);
                }
            }
        } else {
            // Invalid URL format
            if !matches!(pool.status, PoolStatus::Offline) {
                pool.status = PoolStatus::Offline;
                error!("Pool {} has invalid URL format", pool.name);
            }
        }
    }

    drop(pools);

    // Also check user-created pools
    let mut user_pools = USER_CREATED_POOLS.lock().await;
    for pool in user_pools.iter_mut() {
        if let Ok((host, port)) = validate_pool_url(&pool.url) {
            let is_reachable = check_pool_connectivity(&host, port).await;

            if is_reachable {
                if matches!(pool.status, PoolStatus::Offline) {
                    pool.status = PoolStatus::Active;
                }
            } else {
                if !matches!(pool.status, PoolStatus::Offline) {
                    pool.status = PoolStatus::Offline;
                }
            }
        }
    }

    Ok(())
}
