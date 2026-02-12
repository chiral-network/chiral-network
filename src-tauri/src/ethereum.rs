use crate::config::{CHAIN_ID, NETWORK_ID};
use chrono;
use ethers::prelude::*;
use once_cell::sync::Lazy;
use rand::rngs::OsRng;
use secp256k1::{PublicKey, Secp256k1, SecretKey};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha3::{Digest, Keccak256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::Emitter;
use tokio::sync::Mutex;
use url::Url;

// ============================================================================
// Configuration & Shared Resources
// ============================================================================

/// Block reward in Chiral - the amount awarded for mining a block.
/// This is the single source of truth for block rewards throughout the codebase.
pub const BLOCK_REWARD: f64 = 2.0;

pub const DEFAULT_GETH_MAX_PEERS: u32 = 100;
const DEFAULT_RECOVERY_WINDOW_SECS: u64 = 120;
const DEFAULT_RECOVERY_STAGNATION_SECS: u64 = 20;
const DEFAULT_RECOVERY_COOLDOWN_SECS: u64 = 30;
const DEFAULT_RECOVERY_MAX_ATTEMPTS: u32 = 4;
const DEFAULT_RECOVERY_HEADROOM: u32 = 5;
const DEFAULT_RECOVERY_SEED_CAP: usize = 8;
const DEFAULT_RECOVERY_RPC_TIMEOUT_MS: u64 = 2000;
const DEFAULT_RECOVERY_POST_WAIT_MS: u64 = 2000;
const SAMPLE_HISTORY_CAP: usize = 8;

#[derive(Debug, Clone)]
pub struct NetworkConfig {
    pub rpc_endpoint: String,
    pub chain_id: u64,
    pub network_id: u64,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            rpc_endpoint: "http://127.0.0.1:8545".to_string(),
            chain_id: *CHAIN_ID,
            network_id: *NETWORK_ID,
        }
    }
}

// Global configuration - reads from genesis.json or environment variables
pub static NETWORK_CONFIG: Lazy<NetworkConfig> = Lazy::new(|| NetworkConfig {
    rpc_endpoint: std::env::var("CHIRAL_RPC_ENDPOINT")
        .unwrap_or_else(|_| "http://127.0.0.1:8545".to_string()),
    chain_id: *CHAIN_ID,
    network_id: *NETWORK_ID,
});

// Shared HTTP client for all RPC calls
pub static HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("Failed to create HTTP client")
});

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PeerRecoveryReason {
    Healthy,
    CoolingDown,
    Saturated,
    RpcDown,
    WindowExpired,
    WaitingStagnation,
    Attempting,
    AttemptsExhausted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerCountSample {
    pub peer_count: u32,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerAttemptResult {
    pub source: String,
    pub accepted_targets: usize,
    pub attempted_targets: usize,
    pub error: Option<String>,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerRecoverySnapshot {
    pub reason: PeerRecoveryReason,
    pub peer_count: u32,
    pub min_peers: u32,
    pub max_peers: u32,
    pub window_remaining_ms: u64,
    pub cooldown_remaining_ms: u64,
    pub attempt_idx: u32,
    pub target_source: String,
    pub rpc_ok: bool,
    pub trigger_stagnant: bool,
    pub trigger_saturated: bool,
    pub trigger_healthy: bool,
    pub attempted: bool,
    pub last_samples: Vec<PeerCountSample>,
    pub last_attempt: Option<PeerAttemptResult>,
}

#[derive(Debug, Clone)]
struct PeerRecoveryCfg {
    min_peers: u32,
    max_peers: u32,
    cold_window: Duration,
    stagnation_window: Duration,
    cooldown: Duration,
    max_attempts: u32,
    saturation_headroom: u32,
    seed_cap: usize,
    rpc_timeout: Duration,
    post_wait: Duration,
}

impl PeerRecoveryCfg {
    fn with_min_peers(min_peers: u32) -> Self {
        Self {
            min_peers,
            max_peers: get_configured_max_peers(),
            cold_window: Duration::from_secs(DEFAULT_RECOVERY_WINDOW_SECS),
            stagnation_window: Duration::from_secs(DEFAULT_RECOVERY_STAGNATION_SECS),
            cooldown: Duration::from_secs(DEFAULT_RECOVERY_COOLDOWN_SECS),
            max_attempts: DEFAULT_RECOVERY_MAX_ATTEMPTS,
            saturation_headroom: DEFAULT_RECOVERY_HEADROOM,
            seed_cap: DEFAULT_RECOVERY_SEED_CAP,
            rpc_timeout: Duration::from_millis(DEFAULT_RECOVERY_RPC_TIMEOUT_MS),
            post_wait: Duration::from_millis(DEFAULT_RECOVERY_POST_WAIT_MS),
        }
    }
}

#[derive(Debug)]
struct PeerRecoveryState {
    epoch: u64,
    rpc_ready_at: Option<Instant>,
    cooldown_until: Option<Instant>,
    in_flight: bool,
    attempt_idx: u32,
    seed_cursor: usize,
    rpc_fail_cnt: u32,
    sample_hist: VecDeque<PeerCountSample>,
    last_attempt: Option<PeerAttemptResult>,
}

impl Default for PeerRecoveryState {
    fn default() -> Self {
        Self {
            epoch: 0,
            rpc_ready_at: None,
            cooldown_until: None,
            in_flight: false,
            attempt_idx: 0,
            seed_cursor: 0,
            rpc_fail_cnt: 0,
            sample_hist: VecDeque::with_capacity(SAMPLE_HISTORY_CAP),
            last_attempt: None,
        }
    }
}

static PEER_RECOVERY_STATE: Lazy<Mutex<PeerRecoveryState>> =
    Lazy::new(|| Mutex::new(PeerRecoveryState::default()));

#[derive(Debug, Clone, Copy)]
struct RecoveryEval {
    rpc_ok: bool,
    window_active: bool,
    healthy: bool,
    saturated: bool,
    in_flight: bool,
    cooling_down: bool,
    stagnant: bool,
    attempts_exhausted: bool,
}

fn eval_recovery_reason(inp: RecoveryEval) -> PeerRecoveryReason {
    if !inp.rpc_ok {
        return PeerRecoveryReason::RpcDown;
    }
    if !inp.window_active {
        return PeerRecoveryReason::WindowExpired;
    }
    if inp.healthy {
        return PeerRecoveryReason::Healthy;
    }
    if inp.saturated {
        return PeerRecoveryReason::Saturated;
    }
    if inp.in_flight {
        return PeerRecoveryReason::Attempting;
    }
    if inp.cooling_down {
        return PeerRecoveryReason::CoolingDown;
    }
    if inp.attempts_exhausted {
        return PeerRecoveryReason::AttemptsExhausted;
    }
    if !inp.stagnant {
        return PeerRecoveryReason::WaitingStagnation;
    }
    PeerRecoveryReason::Attempting
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn get_configured_max_peers() -> u32 {
    std::env::var("CHIRAL_GETH_MAX_PEERS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u32>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_GETH_MAX_PEERS)
}

fn parse_rpc_quantity_u32(raw: &str, field: &str) -> Result<u32, String> {
    // Ethereum JSON-RPC quantities are 0x-prefixed hex with no leading zeros
    // (except zero itself as `0x0`).
    if !raw.starts_with("0x") {
        return Err(format!(
            "Invalid {} response: expected hex quantity with 0x prefix, got '{}'",
            field, raw
        ));
    }
    let hex = &raw[2..];
    if hex.is_empty() {
        return Err(format!(
            "Invalid {} response: empty quantity '{}'",
            field, raw
        ));
    }
    if hex.len() > 1 && hex.starts_with('0') {
        return Err(format!(
            "Invalid {} response: quantity has leading zero '{}'",
            field, raw
        ));
    }
    u32::from_str_radix(hex, 16)
        .map_err(|e| format!("Failed to parse {} quantity '{}': {}", field, raw, e))
}

fn parse_seed_enodes_list(raw: &str) -> Result<Vec<String>, String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (idx, entry) in raw.split(',').enumerate() {
        let enode = entry.trim();
        if enode.is_empty() {
            continue;
        }
        if !enode.starts_with("enode://") {
            return Err(format!(
                "Invalid seed enode at index {}: '{}' must start with enode://",
                idx, enode
            ));
        }
        crate::geth_bootstrap::parse_enode_address(enode)
            .map_err(|e| format!("Invalid seed enode at index {}: {}", idx, e))?;
        if seen.insert(enode.to_string()) {
            out.push(enode.to_string());
        }
    }
    Ok(out)
}

fn get_seed_enodes_from_env() -> Result<Vec<String>, String> {
    let raw = match std::env::var("CHIRAL_GETH_SEED_ENODES") {
        Ok(v) => v,
        Err(_) => return Ok(Vec::new()),
    };
    parse_seed_enodes_list(&raw)
}

fn push_peer_sample(samples: &mut VecDeque<PeerCountSample>, peer_count: u32, timestamp_ms: u64) {
    if samples.len() == SAMPLE_HISTORY_CAP {
        samples.pop_front();
    }
    samples.push_back(PeerCountSample {
        peer_count,
        timestamp_ms,
    });
}

fn is_stagnant(samples: &VecDeque<PeerCountSample>, now_ms: u64, span: Duration) -> bool {
    if samples.len() < 2 {
        return false;
    }
    let cutoff = now_ms.saturating_sub(span.as_millis() as u64);
    let newest = match samples.back() {
        Some(v) => v,
        None => return false,
    };
    let baseline = samples
        .iter()
        .rev()
        .find(|sample| sample.timestamp_ms <= cutoff);
    match baseline {
        Some(base) => newest.peer_count <= base.peer_count,
        None => false,
    }
}

fn select_seed_batch(seeds: &[String], cursor: usize, cap: usize) -> (Vec<String>, usize) {
    if seeds.is_empty() {
        return (Vec::new(), cursor);
    }
    let mut idx = cursor % seeds.len();
    let take = cap.min(seeds.len());
    let mut out = Vec::with_capacity(take);
    for _ in 0..take {
        out.push(seeds[idx].clone());
        idx = (idx + 1) % seeds.len();
    }
    (out, idx)
}

//Structs
#[derive(Debug, Serialize, Deserialize)]
pub struct EthAccount {
    pub address: String,
    pub private_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EthSignedMessage {
    pub message: String,
    pub signature: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountInfo {
    pub address: String,
    pub balance: String,
}
//Mined Block Struct to return to frontend
#[derive(Debug, Serialize)]
pub struct MinedBlock {
    pub hash: String,
    pub nonce: Option<String>,
    pub difficulty: Option<String>,
    pub timestamp: u64,
    pub number: u64,
    pub reward: Option<f64>, //Chiral Earned
}

pub struct GethProcess {
    child: Option<Child>,
}

impl GethProcess {
    pub fn new() -> Self {
        GethProcess { child: None }
    }

    /// Returns true if this process was started and is managed by this app instance.
    /// Note: `is_running()` may return true even when unmanaged (e.g., SSH port-forward to a remote RPC),
    /// because it checks whether the configured RPC endpoint is reachable.
    pub fn is_managed(&self) -> bool {
        self.child.is_some()
    }

    pub fn is_running(&self) -> bool {
        // First check if we have a tracked child process
        if self.child.is_some() {
            return true;
        }

        // IMPORTANT:
        // This method is called from async contexts (tauri commands / startup tasks).
        // Using `reqwest::blocking` here can panic ("Cannot drop a runtime...") because it creates
        // an internal runtime that gets dropped on a tokio worker thread. So we keep this check
        // lightweight and runtime-free: parse the URL and do a TCP connect.
        let Ok(url) = Url::parse(&NETWORK_CONFIG.rpc_endpoint) else {
            return false;
        };
        let Some(host) = url.host_str() else {
            return false;
        };
        let port = url.port_or_known_default().unwrap_or(8545);
        let addr = format!("{host}:{port}");
        match addr.parse() {
            Ok(sock_addr) => {
                TcpStream::connect_timeout(&sock_addr, Duration::from_millis(500)).is_ok()
            }
            Err(_) => false,
        }
    }

    fn resolve_data_dir(&self, data_dir: &str) -> Result<PathBuf, String> {
        let dir = PathBuf::from(data_dir);
        if dir.is_absolute() {
            return Ok(dir);
        }
        let exe_dir = std::env::current_exe()
            .map_err(|e| format!("Failed to get exe path: {}", e))?
            .parent()
            .ok_or("Failed to get exe dir")?
            .to_path_buf();
        Ok(exe_dir.join(dir))
    }

    /// Validate that existing Geth data directory has the correct chain ID
    fn validate_chain_id(&self, data_path: &Path, expected_chain_id: u64) -> Result<(), String> {
        // Create a marker file to track which chain ID this data directory was initialized with
        let chain_id_marker = data_path.join("geth").join(".chain_id");

        if chain_id_marker.exists() {
            if let Ok(content) = std::fs::read_to_string(&chain_id_marker) {
                if let Ok(stored_chain_id) = content.trim().parse::<u64>() {
                    if stored_chain_id == expected_chain_id {
                        return Ok(()); // Chain ID matches
                    } else {
                        return Err(format!("Existing blockchain data is for chain ID {}, but expected {}. Will reinitialize.", stored_chain_id, expected_chain_id));
                    }
                }
            }
        }

        // If no marker file exists, we can't be sure about the chain ID
        // To be safe, we'll assume it's wrong and force reinitialization
        // This prevents the chain ID mismatch issue from happening again
        Err(format!("Could not verify chain ID of existing blockchain data. Will reinitialize to ensure correctness."))
    }

    pub fn start(
        &mut self,
        data_dir: &str,
        miner_address: Option<&str>,
        pure_client_mode: bool,
    ) -> Result<(), String> {
        // Check if we already have a tracked child process
        if self.child.is_some() {
            return Ok(()); // Already running, no need to start again
        }

        // Always kill any existing geth processes before starting
        // This ensures we don't have multiple instances running
        // First try to stop via HTTP if it's running
        if self.is_running() {
            let _ = Command::new("curl")
                .arg("-s")
                .arg("-X")
                .arg("POST")
                .arg("-H")
                .arg("Content-Type: application/json")
                .arg("--data")
                .arg(r#"{"jsonrpc":"2.0","method":"admin_stopRPC","params":[],"id":1}"#)
                .arg("http://127.0.0.1:8545")
                .output();
            std::thread::sleep(std::time::Duration::from_millis(500));
        }

        // Gracefully kill any remaining geth processes (SIGTERM allows clean shutdown)
        #[cfg(unix)]
        {
            // Kill by name pattern
            let _ = Command::new("pkill")
                .arg("-15") // SIGTERM - graceful shutdown
                .arg("-f")
                .arg("geth.*--datadir.*geth-data")
                .output();

            // Also try to kill by port usage (macOS compatible)
            let _ = Command::new("sh")
                .arg("-c")
                .arg("lsof -ti:8545,30303 | xargs kill -15 2>/dev/null || true")
                .output();

            // Give Geth time to gracefully shut down and flush database
            std::thread::sleep(std::time::Duration::from_secs(3));
        }

        // Final check - if still running, we have a problem
        if self.is_running() {
            // Try one more aggressive kill
            #[cfg(unix)]
            {
                let _ = Command::new("sh")
                    .arg("-c")
                    .arg("ps aux | grep -E 'geth.*--datadir' | grep -v grep | awk '{print $2}' | xargs kill -9 2>/dev/null || true")
                    .output();
                std::thread::sleep(std::time::Duration::from_millis(500));
            }

            if self.is_running() {
                return Err(
                    "Cannot stop existing geth process. Please manually kill it and try again."
                        .to_string(),
                );
            }
        }

        // Use the GethDownloader to get the correct path
        let downloader = crate::geth_downloader::GethDownloader::new();
        let geth_path = downloader.geth_path();

        if !geth_path.exists() {
            return Err("Geth binary not found. Please download it first.".to_string());
        }

        // Use the project directory as base for genesis.json
        let project_dir = if cfg!(debug_assertions) {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .ok_or("Failed to get project dir")?
                .to_path_buf()
        } else {
            std::env::current_exe()
                .map_err(|e| format!("Failed to get exe path: {}", e))?
                .parent()
                .ok_or("Failed to get parent dir")?
                .parent()
                .ok_or("Failed to get parent dir")?
                .parent()
                .ok_or("Failed to get parent dir")?
                .to_path_buf()
        };

        // Resolve genesis.json path robustly across:
        // - dev builds (repo root)
        // - release/headless builds (binary under src-tauri/target/release/)
        // - custom deployments (env override)
        let genesis_candidates: Vec<PathBuf> = {
            let mut v = Vec::new();
            if let Ok(p) = std::env::var("CHIRAL_GENESIS_PATH") {
                if !p.trim().is_empty() {
                    v.push(PathBuf::from(p.trim()));
                }
            }
            // Common locations
            v.push(project_dir.join("genesis.json"));
            if let Some(parent) = project_dir.parent() {
                v.push(parent.join("genesis.json")); // repo root when project_dir == src-tauri
            }
            if let Ok(cwd) = std::env::current_dir() {
                v.push(cwd.join("genesis.json"));
            }
            v
        };

        let genesis_path = genesis_candidates
            .iter()
            .find(|p| p.exists())
            .cloned()
            .ok_or_else(|| {
                format!(
                    "genesis.json not found. Tried: {}. You can override with CHIRAL_GENESIS_PATH=/path/to/genesis.json",
                    genesis_candidates
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;

        // Resolve data directory relative to the executable dir if it's relative
        let data_path = self.resolve_data_dir(data_dir)?;

        // Check if we need to initialize or reinitialize blockchain
        let needs_init = !data_path.join("geth").exists();
        let mut needs_reinit = false;

        // Check if existing blockchain data has wrong chain ID
        if !needs_init {
            if let Err(chain_mismatch) = self.validate_chain_id(&data_path, *CHAIN_ID) {
                eprintln!("⚠️  Chain ID mismatch detected: {}", chain_mismatch);
                eprintln!("⚠️  Existing blockchain data is for wrong chain, will reinitialize...");
                needs_reinit = true;
            }
        }

        // Check for blockchain corruption by looking at recent logs
        if !needs_init {
            let log_path = data_path.join("geth.log");
            if log_path.exists() {
                // Read last 50 lines of log to check for corruption errors
                if let Ok(file) = File::open(&log_path) {
                    let reader = BufReader::new(file);
                    let all_lines: Vec<String> = reader.lines().filter_map(Result::ok).collect();
                    let lines: Vec<String> = all_lines.iter().rev().take(50).cloned().collect();

                    // Look for signs of ACTUAL blockchain corruption (not normal operations)
                    for line in &lines {
                        if line.contains("database corruption")
                            || line.contains("FATAL") && line.contains("chaindata")
                            || line.contains("corrupted") && line.contains("database")
                        {
                            eprintln!("⚠️  Detected corrupted blockchain, will reinitialize...");
                            needs_reinit = true;
                            break;
                        }
                    }
                }
            }
        }

        // Remove corrupted blockchain data if needed
        if needs_reinit {
            let geth_dir = data_path.join("geth");
            if geth_dir.exists() {
                eprintln!("Removing corrupted blockchain data...");
                std::fs::remove_dir_all(&geth_dir)
                    .map_err(|e| format!("Failed to remove corrupted blockchain: {}", e))?;
            }
        }

        // Initialize with genesis if needed
        if needs_init || needs_reinit {
            eprintln!("Initializing blockchain with genesis block...");
            let init_output = Command::new(&geth_path)
                .arg("--datadir")
                .arg(&data_path)
                .arg("init")
                .arg(&genesis_path)
                .output()
                .map_err(|e| format!("Failed to initialize genesis: {}", e))?;

            if !init_output.status.success() {
                return Err(format!(
                    "Failed to init genesis: {}",
                    String::from_utf8_lossy(&init_output.stderr)
                ));
            }

            // Write a marker file to track which chain ID this data directory was initialized with
            let chain_id_marker = data_path.join("geth").join(".chain_id");
            if let Err(e) = std::fs::write(&chain_id_marker, CHAIN_ID.to_string()) {
                eprintln!("Warning: Failed to write chain ID marker file: {}", e);
            }

            eprintln!(
                "✅ Blockchain initialized successfully with chain ID {}",
                *CHAIN_ID
            );
        }

        // Get bootstrap nodes - use cached/fallback to avoid blocking startup
        // The health check is performed asynchronously, so we use the synchronous fallback here
        // and the async health-checked version will be used for reconnection attempts
        let bootstrap_enode = crate::geth_bootstrap::get_all_bootstrap_enode_string();

        // Log bootstrap configuration
        let node_count = bootstrap_enode.matches("enode://").count();
        eprintln!("Starting Geth with {} bootstrap node(s)", node_count);
        let max_peers = get_configured_max_peers();

        let mut cmd = Command::new(&geth_path);
        cmd.arg("--datadir")
            .arg(&data_path)
            .arg("--networkid")
            .arg(NETWORK_CONFIG.network_id.to_string())
            .arg("--bootnodes")
            .arg(bootstrap_enode)
            .arg("--http")
            .arg("--http.addr")
            .arg("127.0.0.1")
            .arg("--http.port")
            .arg("8545")
            .arg("--http.api")
            .arg("eth,net,web3,personal,debug,miner,admin,txpool")
            .arg("--http.corsdomain")
            .arg("*")
            .arg("--syncmode")
            .arg("snap") // Always use snap mode (light mode doesn't work for private networks)
            // Sync performance optimizations
            .arg("--cache")
            .arg("2048") // Increase cache to 2GB for faster sync (default is 1024)
            .arg("--cache.database")
            .arg("60") // 60% of cache for database
            .arg("--cache.trie")
            .arg("30") // 30% for trie cache
            .arg("--cache.gc")
            .arg("10") // 10% for garbage collection
            .arg("--maxpeers")
            .arg(max_peers.to_string())
            // P2P discovery settings
            .arg("--port")
            .arg("30303") // P2P listening port
            // Network address configuration
            .arg("--nat")
            .arg("any")
            // Snapshot sync acceleration
            .arg("--snapshot")
            .arg("--state.scheme")
            .arg("hash") // Use hash-based state scheme for better performance
            // Enable transaction pool gossip to propagate transactions across network
            .arg("--txpool.globalslots")
            .arg("16384") // Increase tx pool size for network-wide transactions
            .arg("--txpool.globalqueue")
            .arg("4096")
            // Txpool settings to ensure transactions are included
            .arg("--txpool.accountslots")
            .arg("64") // Allow more pending txs per account
            .arg("--txpool.accountqueue")
            .arg("128")
            .arg("--txpool.pricebump")
            .arg("0") // Don't require price bump for replacement
            .arg("--txpool.pricelimit")
            .arg("0") // Accept transactions with 0 gas price
            // Set minimum gas price to 0 to accept all transactions
            // This is important for a private network where we want all transactions to be mined
            .arg("--miner.gasprice")
            .arg("0")
            // Recommend transactions for mining (include pending txs)
            .arg("--miner.recommit")
            .arg("500ms") // Re-create the mining block every 500ms to include new transactions faster
            // Limit transaction lookup to reduce storage (partial blockchain sync)
            .arg("--txlookuplimit")
            .arg(if pure_client_mode { "100" } else { "10000" }); // Pure-client: 100 blocks, Normal: 10000 blocks

        // Add this line to set a shorter IPC path
        cmd.arg("--ipcpath").arg("/tmp/chiral-geth.ipc");

        // Note: We don't set --miner.etherbase here due to Core-Geth parsing issues
        // Instead, we'll set it via RPC call in start_mining()
        // The miner_address will be stored and used when mining starts

        // Create log file for geth output
        let log_path = data_path.join("geth.log");
        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|e| format!("Failed to create log file: {}", e))?;

        let log_file_clone = log_file
            .try_clone()
            .map_err(|e| format!("Failed to clone log file handle: {}", e))?;

        cmd.stdout(Stdio::from(log_file_clone))
            .stderr(Stdio::from(log_file));

        let child = cmd
            .spawn()
            .map_err(|e| format!("Failed to start geth: {}", e))?;

        self.child = Some(child);

        eprintln!("✅ Geth process started successfully");
        eprintln!("    Logs: {}", log_path.display());
        eprintln!("    RPC: http://127.0.0.1:8545");
        eprintln!("    Waiting for RPC to be ready...");

        // Give Geth a moment to start up
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Verify the process is still running (didn't crash immediately)
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    // Process has already exited - something went wrong
                    self.child = None;
                    return Err(format!(
                        "Geth process exited immediately with status: {}. Check logs at: {}",
                        status,
                        log_path.display()
                    ));
                }
                Ok(None) => {
                    // Process is still running - good!
                    eprintln!("✅ Geth is running");
                }
                Err(e) => {
                    eprintln!("⚠️  Warning: Could not check geth process status: {}", e);
                }
            }
        }

        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), String> {
        // First try to kill the tracked child process
        if let Some(mut child) = self.child.take() {
            // Try to kill the process
            match child.kill() {
                Ok(_) => {
                    // Wait for the process to actually exit
                    let _ = child.wait();
                }
                Err(_) => {
                    // Process was already dead or couldn't be killed
                }
            }
        }

        // Always kill any geth processes by name as a fallback
        // This handles orphaned processes
        #[cfg(unix)]
        {
            // Kill by process name with SIGTERM for graceful shutdown
            let result = Command::new("pkill")
                .arg("-15")
                .arg("-f")
                .arg("geth.*--datadir.*geth-data")
                .output();

            match result {
                Ok(_output) => {
                    // pkill completed
                }
                Err(_e) => {
                    // Failed to run pkill
                }
            }

            // Also kill by port usage
            let _ = Command::new("sh")
                .arg("-c")
                .arg("lsof -ti:8545,30303 | xargs kill -15 2>/dev/null || true")
                .output();

            // Give Geth time to gracefully shut down
            std::thread::sleep(std::time::Duration::from_secs(2));
        }

        Ok(())
    }
}

// ============================================================================
// Async Geth Management Functions
// ============================================================================

/// Start Geth with health-checked bootstrap nodes
///
/// This function performs bootstrap node health checks before starting Geth,
/// ensuring we connect to the most reliable nodes available.
pub async fn start_geth_with_health_check(
    geth: &mut GethProcess,
    data_dir: &str,
    miner_address: Option<&str>,
) -> Result<crate::geth_bootstrap::BootstrapHealthReport, String> {
    // First, perform health check on bootstrap nodes
    let health_report = crate::geth_bootstrap::check_all_bootstrap_nodes().await;

    if !health_report.healthy {
        eprintln!(
            "Warning: Bootstrap health check failed - {} of {} nodes reachable",
            health_report.reachable_nodes, health_report.total_nodes
        );
        if let Some(ref recommendation) = health_report.recommendation {
            eprintln!("Recommendation: {}", recommendation);
        }
    } else {
        eprintln!(
            "Bootstrap health OK: {} of {} nodes reachable",
            health_report.reachable_nodes, health_report.total_nodes
        );
    }

    // Start Geth (it will use the fallback bootstrap string if health check found issues)
    geth.start(data_dir, miner_address, false)?; // Use normal snap sync mode

    Ok(health_report)
}

/// Add a new peer to the running Geth node via admin_addPeer RPC
async fn add_peer_with_endpoint(rpc_endpoint: &str, enode: &str) -> Result<bool, String> {
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "admin_addPeer",
        "params": [enode],
        "id": 1
    });

    let response = HTTP_CLIENT
        .post(rpc_endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to send add_peer request: {}", e))?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse add_peer response: {}", e))?;

    if let Some(error) = json_response.get("error") {
        return Err(format!("RPC error adding peer: {}", error));
    }

    Ok(json_response["result"].as_bool().unwrap_or(false))
}

pub async fn add_peer(enode: &str) -> Result<bool, String> {
    add_peer_with_endpoint(&NETWORK_CONFIG.rpc_endpoint, enode).await
}

/// Get list of currently connected peers
pub async fn get_peers() -> Result<Vec<serde_json::Value>, String> {
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "admin_peers",
        "params": [],
        "id": 1
    });

    let response = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to send get_peers request: {}", e))?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse get_peers response: {}", e))?;

    if let Some(error) = json_response.get("error") {
        return Err(format!("RPC error getting peers: {}", error));
    }

    let peers = json_response["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    Ok(peers)
}

async fn get_peer_count_with_timeout(rpc_endpoint: &str, timeout: Duration) -> Result<u32, String> {
    match tokio::time::timeout(timeout, get_peer_count_with_endpoint(rpc_endpoint)).await {
        Ok(res) => res,
        Err(_) => Err(format!(
            "Timed out waiting for net_peerCount response after {}ms",
            timeout.as_millis()
        )),
    }
}

fn is_saturated(peer_count: u32, max_peers: u32, headroom: u32) -> bool {
    let threshold = max_peers.saturating_sub(headroom);
    peer_count >= threshold
}

fn build_snapshot(
    state: &PeerRecoveryState,
    cfg: &PeerRecoveryCfg,
    reason: PeerRecoveryReason,
    peer_count: u32,
    now: Instant,
    target_source: String,
    attempted: bool,
    rpc_ok: bool,
    trigger_stagnant: bool,
) -> PeerRecoverySnapshot {
    let window_remaining_ms = state
        .rpc_ready_at
        .and_then(|started| {
            cfg.cold_window
                .checked_sub(now.saturating_duration_since(started))
        })
        .unwrap_or_default()
        .as_millis() as u64;
    let cooldown_remaining_ms = state
        .cooldown_until
        .and_then(|until| until.checked_duration_since(now))
        .unwrap_or_default()
        .as_millis() as u64;
    PeerRecoverySnapshot {
        reason,
        peer_count,
        min_peers: cfg.min_peers,
        max_peers: cfg.max_peers,
        window_remaining_ms,
        cooldown_remaining_ms,
        attempt_idx: state.attempt_idx,
        target_source,
        rpc_ok,
        trigger_stagnant,
        trigger_saturated: is_saturated(peer_count, cfg.max_peers, cfg.saturation_headroom),
        trigger_healthy: peer_count >= cfg.min_peers,
        attempted,
        last_samples: state.sample_hist.iter().cloned().collect(),
        last_attempt: state.last_attempt.clone(),
    }
}

pub async fn reset_peer_recovery_state() {
    let mut state = PEER_RECOVERY_STATE.lock().await;
    let next_epoch = state.epoch.saturating_add(1);
    *state = PeerRecoveryState {
        epoch: next_epoch,
        ..PeerRecoveryState::default()
    };
}

async fn reconnect_to_bootstrap_with_snapshot_inner(
    min_peers: u32,
    rpc_endpoint: &str,
    seed_enodes_override: Option<Vec<String>>,
) -> Result<PeerRecoverySnapshot, String> {
    let cfg = PeerRecoveryCfg::with_min_peers(min_peers);
    let now = Instant::now();
    let now_ms = now_unix_ms();

    let peer_res = get_peer_count_with_timeout(rpc_endpoint, cfg.rpc_timeout).await;
    let mut state = PEER_RECOVERY_STATE.lock().await;

    let mut peer_count = state.sample_hist.back().map(|v| v.peer_count).unwrap_or(0);
    let rpc_ok = match peer_res {
        Ok(cnt) => {
            peer_count = cnt;
            state.rpc_fail_cnt = 0;
            if state.rpc_ready_at.is_none() {
                state.rpc_ready_at = Some(now);
            }
            push_peer_sample(&mut state.sample_hist, cnt, now_ms);
            true
        }
        Err(err) => {
            state.rpc_fail_cnt = state.rpc_fail_cnt.saturating_add(1);
            if state.sample_hist.is_empty() {
                push_peer_sample(&mut state.sample_hist, 0, now_ms);
            }
            state.last_attempt = Some(PeerAttemptResult {
                source: "none".to_string(),
                accepted_targets: 0,
                attempted_targets: 0,
                error: Some(err),
                timestamp_ms: now_ms,
            });
            let reason = eval_recovery_reason(RecoveryEval {
                rpc_ok: false,
                window_active: true,
                healthy: false,
                saturated: false,
                in_flight: state.in_flight,
                cooling_down: false,
                stagnant: false,
                attempts_exhausted: false,
            });
            return Ok(build_snapshot(
                &state,
                &cfg,
                reason,
                peer_count,
                now,
                "none".to_string(),
                false,
                false,
                false,
            ));
        }
    };

    let window_active = state
        .rpc_ready_at
        .map(|started| now.saturating_duration_since(started) <= cfg.cold_window)
        .unwrap_or(false);
    let healthy = peer_count >= cfg.min_peers;
    let saturated = is_saturated(peer_count, cfg.max_peers, cfg.saturation_headroom);
    let cooling_down = state
        .cooldown_until
        .map(|until| now < until)
        .unwrap_or(false);
    let stagnant = is_stagnant(&state.sample_hist, now_ms, cfg.stagnation_window);
    let attempts_exhausted = state.attempt_idx >= cfg.max_attempts;

    if state.in_flight {
        return Ok(build_snapshot(
            &state,
            &cfg,
            PeerRecoveryReason::Attempting,
            peer_count,
            now,
            "none".to_string(),
            false,
            true,
            stagnant,
        ));
    }

    let reason = eval_recovery_reason(RecoveryEval {
        rpc_ok,
        window_active,
        healthy,
        saturated,
        in_flight: false,
        cooling_down,
        stagnant,
        attempts_exhausted,
    });

    if reason != PeerRecoveryReason::Attempting {
        return Ok(build_snapshot(
            &state,
            &cfg,
            reason,
            peer_count,
            now,
            "none".to_string(),
            false,
            true,
            stagnant,
        ));
    }

    let seed_enodes = match seed_enodes_override {
        Some(v) => v,
        None => get_seed_enodes_from_env()?,
    };
    let (targets, target_source, next_cursor) = if !seed_enodes.is_empty() {
        let (picked, next) = select_seed_batch(&seed_enodes, state.seed_cursor, cfg.seed_cap);
        (picked, "seed".to_string(), next)
    } else {
        let healthy_enodes = crate::geth_bootstrap::get_healthy_bootstrap_enode_string().await;
        let mut fallback: Vec<String> = healthy_enodes
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        fallback.dedup();
        let (picked, next) = select_seed_batch(&fallback, state.seed_cursor, cfg.seed_cap);
        (
            picked,
            "bootstrap_fallback".to_string(),
            next,
        )
    };

    if targets.is_empty() {
        state.last_attempt = Some(PeerAttemptResult {
            source: target_source.clone(),
            accepted_targets: 0,
            attempted_targets: 0,
            error: Some("No recovery targets available".to_string()),
            timestamp_ms: now_ms,
        });
        return Ok(build_snapshot(
            &state,
            &cfg,
            PeerRecoveryReason::WaitingStagnation,
            peer_count,
            now,
            target_source,
            false,
            true,
            stagnant,
        ));
    }

    state.in_flight = true;
    state.attempt_idx = state.attempt_idx.saturating_add(1);
    state.seed_cursor = next_cursor;
    let attempt_epoch = state.epoch;
    let attempt_idx = state.attempt_idx;
    drop(state);

    let mut accepted = 0usize;
    let mut err_msg: Option<String> = None;
    for enode in &targets {
        match tokio::time::timeout(cfg.rpc_timeout, add_peer_with_endpoint(rpc_endpoint, enode)).await {
            Ok(Ok(true)) => accepted += 1,
            Ok(Ok(false)) => {}
            Ok(Err(e)) => err_msg = Some(e),
            Err(_) => {
                err_msg = Some(format!(
                    "Timed out waiting for admin_addPeer response after {}ms",
                    cfg.rpc_timeout.as_millis()
                ));
            }
        }
    }

    tokio::time::sleep(cfg.post_wait).await;
    let post_peer_count = get_peer_count_with_timeout(rpc_endpoint, cfg.rpc_timeout)
        .await
        .unwrap_or(peer_count);
    let post_ms = now_unix_ms();

    let mut state = PEER_RECOVERY_STATE.lock().await;
    if state.epoch != attempt_epoch {
        let current_peer_count = state.sample_hist.back().map(|v| v.peer_count).unwrap_or(post_peer_count);
        let stagnant_now = is_stagnant(&state.sample_hist, post_ms, cfg.stagnation_window);
        return Ok(build_snapshot(
            &state,
            &cfg,
            PeerRecoveryReason::WindowExpired,
            current_peer_count,
            Instant::now(),
            "none".to_string(),
            false,
            true,
            stagnant_now,
        ));
    }
    state.in_flight = false;
    state.cooldown_until = Some(Instant::now() + cfg.cooldown);
    push_peer_sample(&mut state.sample_hist, post_peer_count, post_ms);
    state.last_attempt = Some(PeerAttemptResult {
        source: target_source.clone(),
        accepted_targets: accepted,
        attempted_targets: targets.len(),
        error: err_msg,
        timestamp_ms: post_ms,
    });

    let post_reason = if post_peer_count >= cfg.min_peers {
        PeerRecoveryReason::Healthy
    } else if attempt_idx >= cfg.max_attempts {
        PeerRecoveryReason::AttemptsExhausted
    } else {
        PeerRecoveryReason::CoolingDown
    };

    Ok(build_snapshot(
        &state,
        &cfg,
        post_reason,
        post_peer_count,
        Instant::now(),
        target_source,
        true,
        true,
        true,
    ))
}

pub async fn reconnect_to_bootstrap_with_snapshot(
    min_peers: u32,
) -> Result<PeerRecoverySnapshot, String> {
    reconnect_to_bootstrap_with_snapshot_inner(min_peers, &NETWORK_CONFIG.rpc_endpoint, None).await
}

/// Reconnect to peer candidates if geth peer count is low.
///
/// This keeps compatibility with existing command callers that expect a peer count.
pub async fn reconnect_to_bootstrap_if_needed(min_peers: u32) -> Result<u32, String> {
    let snapshot = reconnect_to_bootstrap_with_snapshot(min_peers).await?;
    Ok(snapshot.peer_count)
}

/// Get Geth node info including enode
pub async fn get_node_info() -> Result<serde_json::Value, String> {
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "admin_nodeInfo",
        "params": [],
        "id": 1
    });

    let response = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to send nodeInfo request: {}", e))?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse nodeInfo response: {}", e))?;

    if let Some(error) = json_response.get("error") {
        return Err(format!("RPC error getting node info: {}", error));
    }

    Ok(json_response["result"].clone())
}

impl Drop for GethProcess {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn public_key_to_address(public_key: &PublicKey) -> String {
    let public_key_bytes = public_key.serialize_uncompressed();
    // Skip the first byte (0x04) which is the uncompressed prefix
    let hash = Keccak256::digest(&public_key_bytes[1..]);
    // Take the last 20 bytes of the hash
    let address_bytes = &hash[12..];
    format!("0x{}", hex::encode(address_bytes))
}

pub fn create_new_account() -> Result<EthAccount, String> {
    let secp = Secp256k1::new();
    let (secret_key, public_key) = secp.generate_keypair(&mut OsRng);

    let address = public_key_to_address(&public_key);
    let private_key = hex::encode(secret_key.as_ref());

    Ok(EthAccount {
        address,
        private_key,
    })
}

pub fn get_account_from_private_key(private_key_hex: &str) -> Result<EthAccount, String> {
    let secp = Secp256k1::new();

    // Remove 0x prefix if present
    let private_key_hex = if private_key_hex.starts_with("0x") {
        &private_key_hex[2..]
    } else {
        private_key_hex
    };

    let private_key_bytes =
        hex::decode(private_key_hex).map_err(|e| format!("Invalid hex private key: {}", e))?;

    let secret_key = SecretKey::from_slice(&private_key_bytes)
        .map_err(|e| format!("Invalid private key: {}", e))?;

    let public_key = PublicKey::from_secret_key(&secp, &secret_key);
    let address = public_key_to_address(&public_key);

    Ok(EthAccount {
        address,
        private_key: private_key_hex.to_string(),
    })
}

pub async fn get_balance(address: &str) -> Result<String, String> {
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_getBalance",
        "params": [address, "latest"],
        "id": 1
    });

    let response = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to send request: {}", e))?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    if let Some(error) = json_response.get("error") {
        return Err(format!("RPC error: {}", error));
    }

    let balance_hex = json_response["result"]
        .as_str()
        .ok_or("Invalid balance response")?;

    // Convert hex to decimal (wei)
    let balance_wei = u128::from_str_radix(&balance_hex[2..], 16)
        .map_err(|e| format!("Failed to parse balance: {}", e))?;

    // Convert wei to ether (1 ether = 10^18 wei)
    let balance_ether = balance_wei as f64 / 1e18;

    tracing::debug!(
        "💰 Balance for {}: {} (raw: {})",
        address,
        balance_ether,
        balance_hex
    );

    Ok(format!("{:.6}", balance_ether))
}

async fn get_peer_count_with_endpoint(rpc_endpoint: &str) -> Result<u32, String> {
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "net_peerCount",
        "params": [],
        "id": 1
    });

    let response = HTTP_CLIENT
        .post(rpc_endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to send request: {}", e))?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    if let Some(error) = json_response.get("error") {
        return Err(format!("RPC error: {}", error));
    }

    let peer_count_hex = json_response["result"]
        .as_str()
        .ok_or("Invalid peer count response")?;

    let peer_count = parse_rpc_quantity_u32(peer_count_hex, "peer count")?;

    Ok(peer_count)
}

pub async fn get_peer_count() -> Result<u32, String> {
    get_peer_count_with_endpoint(&NETWORK_CONFIG.rpc_endpoint).await
}

/// Get the chain ID from the running Geth node via RPC
pub async fn get_chain_id() -> Result<u64, String> {
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_chainId",
        "params": [],
        "id": 1
    });

    let response = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to send request: {}", e))?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    if let Some(error) = json_response.get("error") {
        return Err(format!("RPC error: {}", error));
    }

    let chain_id_hex = json_response["result"]
        .as_str()
        .ok_or("Invalid chain ID response")?;

    // Convert hex to decimal (strip 0x prefix)
    let chain_id = u64::from_str_radix(&chain_id_hex[2..], 16)
        .map_err(|e| format!("Failed to parse chain ID: {}", e))?;

    Ok(chain_id)
}

pub async fn start_mining(miner_address: &str, threads: u32) -> Result<(), String> {
    // First, ensure geth is ready to accept RPC calls
    let mut attempts = 0;
    let max_attempts = 60; // 60 seconds max wait (Geth may need time to rewind blockchain)
    loop {
        // Check if geth is responding to RPC calls
        if let Ok(response) = HTTP_CLIENT
            .post(&NETWORK_CONFIG.rpc_endpoint)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "net_version",
                "params": [],
                "id": 1
            }))
            .send()
            .await
        {
            if response.status().is_success() {
                if let Ok(json) = response.json::<serde_json::Value>().await {
                    if json.get("result").is_some() {
                        break; // Geth is ready
                    }
                }
            }
        }

        attempts += 1;
        if attempts >= max_attempts {
            return Err(
                "Geth RPC endpoint is not responding. Please ensure the Chiral node is running."
                    .to_string(),
            );
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }

    tracing::info!("🔧 Setting up mining with etherbase: {}", miner_address);

    // Ensure address has 0x prefix and is lowercase for consistency
    let formatted_address = if miner_address.starts_with("0x") {
        miner_address.to_lowercase()
    } else {
        format!("0x{}", miner_address.to_lowercase())
    };

    // First try to set the etherbase using miner_setEtherbase
    let set_etherbase = json!({
        "jsonrpc": "2.0",
        "method": "miner_setEtherbase",
        "params": [formatted_address],
        "id": 1
    });

    let response = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&set_etherbase)
        .send()
        .await
        .map_err(|e| format!("Failed to set etherbase: {}", e))?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    // Check if setting etherbase worked
    if let Some(error) = json_response.get("error") {
        eprintln!("Could not set etherbase via RPC: {}", error);
        // Return error to trigger restart
        return Err(format!("{}", error));
    }

    // Now start mining with the specified threads
    let start_mining = json!({
        "jsonrpc": "2.0",
        "method": "miner_start",
        "params": [threads],
        "id": 2
    });

    let response = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&start_mining)
        .send()
        .await
        .map_err(|e| format!("Failed to start mining: {}", e))?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    if let Some(error) = json_response.get("error") {
        return Err(format!("{}", error));
    }

    // Log the actual coinbase being used
    match get_coinbase().await {
        Ok(coinbase) => {
            tracing::info!("⛏️ Mining started! Rewards will go to: {}", coinbase);
            if coinbase.to_lowercase() != miner_address.to_lowercase() {
                tracing::warn!(
                    "⚠️ WARNING: Coinbase {} does not match requested miner address {}!",
                    coinbase,
                    miner_address
                );
            }
        }
        Err(e) => tracing::warn!("Could not verify coinbase: {}", e),
    }

    Ok(())
}

pub async fn stop_mining() -> Result<(), String> {
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "miner_stop",
        "params": [],
        "id": 1
    });

    let response = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to stop mining: {}", e))?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    if let Some(error) = json_response.get("error") {
        return Err(format!("Failed to stop mining: {}", error));
    }

    Ok(())
}

pub async fn get_mining_status() -> Result<bool, String> {
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_mining",
        "params": [],
        "id": 1
    });

    let response = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to get mining status: {}", e))?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    if let Some(error) = json_response.get("error") {
        return Err(format!("RPC error: {}", error));
    }

    let is_mining = json_response["result"]
        .as_bool()
        .ok_or("Invalid mining status response")?;

    Ok(is_mining)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SyncStatus {
    pub syncing: bool,
    pub current_block: u64,
    pub highest_block: u64,
    pub starting_block: u64,
    pub progress_percent: f64,
    pub blocks_remaining: u64,
    pub estimated_seconds_remaining: Option<u64>,
}

pub async fn get_sync_status() -> Result<SyncStatus, String> {
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_syncing",
        "params": [],
        "id": 1
    });

    let response = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to get sync status: {}", e))?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    if let Some(error) = json_response.get("error") {
        return Err(format!("RPC error: {}", error));
    }

    // eth_syncing returns false if not syncing, or an object if syncing
    let result = &json_response["result"];

    if result.is_boolean() && result.as_bool() == Some(false) {
        // Not syncing - fully synced
        let block_number = get_block_number().await?;
        return Ok(SyncStatus {
            syncing: false,
            current_block: block_number,
            highest_block: block_number,
            starting_block: block_number,
            progress_percent: 100.0,
            blocks_remaining: 0,
            estimated_seconds_remaining: Some(0),
        });
    }

    // Parse sync progress
    let current_hex = result["currentBlock"]
        .as_str()
        .ok_or("Missing currentBlock")?;
    let highest_hex = result["highestBlock"]
        .as_str()
        .ok_or("Missing highestBlock")?;
    let starting_hex = result["startingBlock"]
        .as_str()
        .ok_or("Missing startingBlock")?;

    let current_block = u64::from_str_radix(current_hex.trim_start_matches("0x"), 16)
        .map_err(|e| format!("Invalid currentBlock: {}", e))?;
    let highest_block = u64::from_str_radix(highest_hex.trim_start_matches("0x"), 16)
        .map_err(|e| format!("Invalid highestBlock: {}", e))?;
    let starting_block = u64::from_str_radix(starting_hex.trim_start_matches("0x"), 16)
        .map_err(|e| format!("Invalid startingBlock: {}", e))?;

    let blocks_remaining = highest_block.saturating_sub(current_block);

    // Calculate progress as percentage of total blockchain synced (current / highest)
    // This gives a more intuitive progress indicator than (current - starting) / (highest - starting)
    let progress_percent = if highest_block > 0 {
        let raw_progress = (current_block as f64 / highest_block as f64) * 100.0;
        raw_progress.min(100.0).max(0.0)
    } else {
        // No blocks yet, consider fully synced
        100.0
    };

    // Estimate time remaining based on ~850 blocks per 8 seconds (observed rate)
    let estimated_seconds_remaining = if blocks_remaining > 0 {
        Some((blocks_remaining as f64 / 850.0 * 8.0) as u64)
    } else {
        Some(0)
    };

    // Consider synced if progress is 100% or no blocks remaining
    let is_still_syncing = blocks_remaining > 0 && progress_percent < 100.0;

    Ok(SyncStatus {
        syncing: is_still_syncing,
        current_block,
        highest_block,
        starting_block,
        progress_percent,
        blocks_remaining,
        estimated_seconds_remaining,
    })
}

pub async fn get_hashrate() -> Result<String, String> {
    // First try eth_hashrate
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_hashrate",
        "params": [],
        "id": 1
    });

    let response = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to get hashrate: {}", e))?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    if let Some(error) = json_response.get("error") {
        // If eth_hashrate fails, try miner_hashrate as fallback
        let miner_payload = json!({
            "jsonrpc": "2.0",
            "method": "miner_hashrate",
            "params": [],
            "id": 1
        });

        if let Ok(miner_response) = HTTP_CLIENT
            .post(&NETWORK_CONFIG.rpc_endpoint)
            .json(&miner_payload)
            .send()
            .await
        {
            if let Ok(miner_json) = miner_response.json::<serde_json::Value>().await {
                if miner_json.get("error").is_none() {
                    // Use miner_hashrate result instead
                    if let Some(result) = miner_json.get("result") {
                        if let Some(hashrate_hex) = result.as_str() {
                            // Process with the same logic below
                            let hex_str = if hashrate_hex.starts_with("0x")
                                || hashrate_hex.starts_with("0X")
                            {
                                &hashrate_hex[2..]
                            } else {
                                hashrate_hex
                            };

                            let hashrate = if hex_str.is_empty() || hex_str == "0" {
                                0
                            } else {
                                u64::from_str_radix(hex_str, 16).unwrap_or(0)
                            };

                            let formatted = if hashrate >= 1_000_000_000 {
                                format!("{:.2} GH/s", hashrate as f64 / 1_000_000_000.0)
                            } else if hashrate >= 1_000_000 {
                                format!("{:.2} MH/s", hashrate as f64 / 1_000_000.0)
                            } else if hashrate >= 1_000 {
                                format!("{:.2} KH/s", hashrate as f64 / 1_000.0)
                            } else {
                                format!("{} H/s", hashrate)
                            };

                            return Ok(formatted);
                        }
                    }
                }
            }
        }

        // If both fail, try one more method: miner.getHashrate
        let gethashrate_payload = json!({
            "jsonrpc": "2.0",
            "method": "miner.getHashrate",
            "params": [],
            "id": 1
        });

        if let Ok(gethashrate_response) = HTTP_CLIENT
            .post(&NETWORK_CONFIG.rpc_endpoint)
            .json(&gethashrate_payload)
            .send()
            .await
        {
            if let Ok(gethashrate_json) = gethashrate_response.json::<serde_json::Value>().await {
                if gethashrate_json.get("error").is_none() {
                    if let Some(result) = gethashrate_json.get("result") {
                        if let Some(hashrate_hex) = result.as_str() {
                            // Process with the same logic below
                            let hex_str = if hashrate_hex.starts_with("0x")
                                || hashrate_hex.starts_with("0X")
                            {
                                &hashrate_hex[2..]
                            } else {
                                hashrate_hex
                            };

                            let hashrate = if hex_str.is_empty() || hex_str == "0" {
                                0
                            } else {
                                u64::from_str_radix(hex_str, 16).unwrap_or(0)
                            };

                            let formatted = if hashrate >= 1_000_000_000 {
                                format!("{:.2} GH/s", hashrate as f64 / 1_000_000_000.0)
                            } else if hashrate >= 1_000_000 {
                                format!("{:.2} MH/s", hashrate as f64 / 1_000_000.0)
                            } else if hashrate >= 1_000 {
                                format!("{:.2} KH/s", hashrate as f64 / 1_000.0)
                            } else {
                                format!("{} H/s", hashrate)
                            };

                            return Ok(formatted);
                        }
                    }
                }
            }
        }

        // If all methods fail, return original error
        return Err(format!("RPC error: {}", error));
    }

    let hashrate_hex = json_response["result"]
        .as_str()
        .ok_or("Invalid hashrate response")?;

    // Handle edge cases where hashrate might be "0x0" or invalid
    let hex_str = if hashrate_hex.starts_with("0x") || hashrate_hex.starts_with("0X") {
        &hashrate_hex[2..]
    } else {
        hashrate_hex
    };

    // Convert hex to decimal, handle empty string or just "0"
    let hashrate = if hex_str.is_empty() || hex_str == "0" {
        0
    } else {
        u64::from_str_radix(hex_str, 16).map_err(|e| format!("Failed to parse hashrate: {}", e))?
    };

    // Convert to human-readable format (H/s, KH/s, MH/s, GH/s)
    let formatted = if hashrate >= 1_000_000_000 {
        format!("{:.2} GH/s", hashrate as f64 / 1_000_000_000.0)
    } else if hashrate >= 1_000_000 {
        format!("{:.2} MH/s", hashrate as f64 / 1_000_000.0)
    } else if hashrate >= 1_000 {
        format!("{:.2} KH/s", hashrate as f64 / 1_000.0)
    } else {
        format!("{} H/s", hashrate)
    };

    Ok(formatted)
}

pub async fn get_block_number() -> Result<u64, String> {
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_blockNumber",
        "params": [],
        "id": 1
    });

    let response = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to get block number: {}", e))?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    if let Some(error) = json_response.get("error") {
        return Err(format!("RPC error: {}", error));
    }

    let block_hex = json_response["result"]
        .as_str()
        .ok_or("Invalid block number response")?;

    // Convert hex to decimal
    let block_number = u64::from_str_radix(&block_hex[2..], 16)
        .map_err(|e| format!("Failed to parse block number: {}", e))?;

    Ok(block_number)
}

pub async fn get_network_difficulty() -> Result<String, String> {
    // Get the latest block to extract difficulty
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_getBlockByNumber",
        "params": ["latest", false],
        "id": 1
    });

    let response = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to get block: {}", e))?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    if let Some(error) = json_response.get("error") {
        return Err(format!("RPC error: {}", error));
    }

    let difficulty_hex = json_response["result"]["difficulty"]
        .as_str()
        .ok_or("Invalid difficulty response")?;

    // Convert hex to decimal
    let difficulty = u128::from_str_radix(&difficulty_hex[2..], 16)
        .map_err(|e| format!("Failed to parse difficulty: {}", e))?;

    // Format difficulty for display
    let formatted = if difficulty >= 1_000_000_000_000 {
        format!("{:.2}T", difficulty as f64 / 1_000_000_000_000.0)
    } else if difficulty >= 1_000_000_000 {
        format!("{:.2}G", difficulty as f64 / 1_000_000_000.0)
    } else if difficulty >= 1_000_000 {
        format!("{:.2}M", difficulty as f64 / 1_000_000.0)
    } else if difficulty >= 1_000 {
        format!("{:.2}K", difficulty as f64 / 1_000.0)
    } else {
        format!("{}", difficulty)
    };

    Ok(formatted)
}

pub async fn get_network_difficulty_as_u64() -> Result<u64, String> {
    let client = reqwest::Client::new();

    // Get the latest block to extract difficulty
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_getBlockByNumber",
        "params": ["latest", false],
        "id": 1
    });

    let response = client
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to get block: {}", e))?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    if let Some(error) = json_response.get("error") {
        return Err(format!("RPC error: {}", error));
    }

    let difficulty_hex = json_response["result"]["difficulty"]
        .as_str()
        .ok_or("Invalid difficulty response")?;

    // Convert hex to decimal
    let hex_str = if difficulty_hex.starts_with("0x") || difficulty_hex.starts_with("0X") {
        &difficulty_hex[2..]
    } else {
        difficulty_hex
    };

    let difficulty = if hex_str.is_empty() {
        1u64
    } else {
        u64::from_str_radix(hex_str, 16).unwrap_or(1)
    };

    Ok(difficulty)
}

// Static storage for mining session data
static MINING_SESSION_DATA: std::sync::Mutex<MiningSessionData> =
    std::sync::Mutex::new(MiningSessionData {
        last_hash_rate: 0.0,
        session_start: 0,
        total_attempts: 0,
    });

#[derive(Clone, Debug)]
struct MiningSessionData {
    last_hash_rate: f64,
    session_start: u64,
    total_attempts: u64,
}

// Helper function to write debug messages to mining logs
fn log_to_mining_logs(data_dir: &str, message: &str) {
    // Resolve data directory
    let exe_dir = match std::env::current_exe() {
        Ok(exe) => match exe.parent() {
            Some(parent) => parent.to_path_buf(),
            None => {
                return;
            }
        },
        Err(_) => {
            return;
        }
    };

    let data_path = if Path::new(data_dir).is_absolute() {
        PathBuf::from(data_dir)
    } else {
        exe_dir.join(data_dir)
    };

    let log_path = data_path.join("geth.log");

    // Create log directory if it doesn't exist
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // Append message to log file
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&log_path) {
        let timestamp = chrono::Utc::now().format("%Y-%m-%d|%H:%M:%S");
        let log_line = format!("INFO [{}] DEBUG: {}\n", timestamp, message);
        let _ = file.write_all(log_line.as_bytes());
    }
}

// Helper function to parse elapsed time strings like "386.043ms", "5.382s", etc.
fn parse_elapsed_time(time_str: &str) -> Result<f64, String> {
    if time_str.ends_with("ms") {
        // Convert milliseconds to seconds
        let ms_str = &time_str[..time_str.len() - 2];
        ms_str
            .parse::<f64>()
            .map(|ms| ms / 1000.0)
            .map_err(|e| format!("Failed to parse milliseconds: {}", e))
    } else if time_str.ends_with('s') {
        // Already in seconds
        let s_str = &time_str[..time_str.len() - 1];
        s_str
            .parse::<f64>()
            .map_err(|e| format!("Failed to parse seconds: {}", e))
    } else {
        // Assume seconds if no unit
        time_str
            .parse::<f64>()
            .map_err(|e| format!("Failed to parse time: {}", e))
    }
}

pub async fn get_mining_performance(data_dir: &str) -> Result<(u64, f64), String> {
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // First, try to get the current network difficulty
    let network_difficulty = match get_network_difficulty_as_u64().await {
        Ok(diff) => diff,
        Err(_) => 1u64, // fallback if RPC fails
    };

    // Resolve relative data_dir against the executable directory
    let exe_dir = std::env::current_exe()
        .map_err(|e| format!("Failed to get exe path: {}", e))?
        .parent()
        .ok_or("Failed to get exe dir")?
        .to_path_buf();
    let data_path = if Path::new(data_dir).is_absolute() {
        PathBuf::from(data_dir)
    } else {
        exe_dir.join(data_dir)
    };
    // Try to get blocks mined from logs first
    let log_path = data_path.join("geth.log");

    // If log doesn't exist, return defaults (blocks will be calculated from balance in frontend)
    if !log_path.exists() {
        // Return 0 for blocks and hash rate - no simulation
        return Ok((0, 0.0));
    }

    let file = File::open(&log_path).map_err(|e| format!("Failed to open log file: {}", e))?;
    let reader = BufReader::new(file);

    let mut blocks_mined = 0u64;
    let mut recent_hashrates = Vec::new();

    // Read last 2000 lines to get recent mining performance
    let lines: Vec<String> = reader
        .lines()
        .filter_map(Result::ok)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .take(2000)
        .collect();

    for line in &lines {
        // Look for various block mining success patterns
        if line.contains("Successfully sealed new block")
            || line.contains("🔨 mined potential block")
            || line.contains("Block mined")
            || (line.contains("mined") && line.contains("block"))
        {
            blocks_mined += 1;
        }

        // Look for mining stats in logs - try multiple patterns
        // Geth may log hashrate in various formats
        let has_hashrate_keyword =
            line.contains("hashrate") || line.contains("hash rate") || line.contains("Hashrate");
        let has_mining_keyword =
            line.contains("Mining") || line.contains("mining") || line.contains("miner");

        if has_hashrate_keyword {
            // Try to extract hashrate if it's explicitly logged
            if let Some(hr_pos) = line.find("hashrate=") {
                let hr_str = &line[hr_pos + 9..];
                if let Some(end_pos) = hr_str.find(|c: char| c == ' ' || c == '\n') {
                    let rate_str = &hr_str[..end_pos];
                    if let Ok(rate) = rate_str.parse::<f64>() {
                        recent_hashrates.push(rate);
                    }
                }
            }
        }

        // Also look for lines with mining performance data
        if has_mining_keyword
            && (line.contains("H/s")
                || line.contains("KH/s")
                || line.contains("MH/s")
                || line.contains("GH/s"))
        {
            // Try to extract hash rate with units
            // Look for patterns like "123.45 MH/s" or "123.45MH/s"
            if let Some(hs_pos) = line.find("H/s") {
                // Look backwards for the number
                let before_hs = &line[..hs_pos];
                if let Some(last_space) = before_hs.rfind(' ') {
                    let rate_str = &before_hs[last_space + 1..];
                    if let Ok(rate) = rate_str.parse::<f64>() {
                        // Convert based on units
                        let multiplier = if line.contains("KH/s") {
                            1000.0
                        } else if line.contains("MH/s") {
                            1_000_000.0
                        } else if line.contains("GH/s") {
                            1_000_000_000.0
                        } else {
                            1.0
                        }; // H/s
                        let actual_rate = rate * multiplier;
                        recent_hashrates.push(actual_rate);
                    }
                }
            }
        }
    }

    // If we found explicit hashrates in logs, use the average
    if !recent_hashrates.is_empty() {
        let avg_hashrate = recent_hashrates.iter().sum::<f64>() / recent_hashrates.len() as f64;
        return Ok((blocks_mined, avg_hashrate));
    }

    // Try to estimate hash rate from recent mining activity
    // Look for recent blocks with elapsed time information
    let mut recent_blocks = Vec::new();

    for line in &lines {
        if line.contains("Successfully sealed new block") && line.contains("elapsed=") {
            // Extract elapsed time for this block
            if let Some(elapsed_pos) = line.find("elapsed=") {
                let elapsed_str = &line[elapsed_pos + 8..];
                if let Some(end_pos) = elapsed_str.find(|c: char| c == ' ' || c == 's' || c == '\n')
                {
                    let time_str = &elapsed_str[..end_pos];
                    if let Ok(elapsed_seconds) = parse_elapsed_time(time_str) {
                        // Also try to get difficulty if available
                        let mut difficulty = network_difficulty; // Use network difficulty as default

                        if let Some(diff_pos) = line.find("diff=") {
                            let diff_str = &line[diff_pos + 5..];
                            if let Some(diff_end) = diff_str.find(|c: char| c == ' ' || c == '\n') {
                                let diff_val_str = &diff_str[..diff_end];
                                if let Ok(diff) = diff_val_str.parse::<u64>() {
                                    difficulty = diff;
                                }
                            }
                        }
                        recent_blocks.push((elapsed_seconds, difficulty));
                    }
                }
            }
        }
    }

    // Update session data
    let mut session_data = match MINING_SESSION_DATA.lock() {
        Ok(data) => data.clone(),
        Err(_) => MiningSessionData {
            last_hash_rate: 0.0,
            session_start: current_time,
            total_attempts: 0,
        },
    };

    // Initialize session start if not set
    if session_data.session_start == 0 {
        session_data.session_start = current_time;
    }

    // Calculate hash rate from mining difficulty and elapsed time (when blocks are found)
    let mut calculated_hashrate = None;
    if !recent_blocks.is_empty() {
        // For each block, hash_rate = difficulty / time_taken
        // Average across all recent blocks for stability
        let mut total_hashrate = 0.0;
        let mut valid_blocks = 0;

        for (elapsed_seconds, difficulty) in &recent_blocks {
            if *elapsed_seconds > 0.0 && *difficulty > 0 {
                let block_hashrate = *difficulty as f64 / *elapsed_seconds;
                total_hashrate += block_hashrate;
                valid_blocks += 1;
            }
        }

        if valid_blocks > 0 {
            let avg_hashrate = total_hashrate / valid_blocks as f64;
            session_data.last_hash_rate = avg_hashrate;
            calculated_hashrate = Some(avg_hashrate);
        }
    }

    // Try to estimate real-time hash rate from mining activity patterns
    // Count recent mining operations in the logs
    let mining_operations = 0;
    for line in &lines {
        // Look for various mining activity indicators
        if line.contains("Commit new sealing work")
            || line.contains("Generating DAG")
            || line.contains("mining")
            || (line.contains("miner") && line.contains("start"))
        {
            // Mining operation detected
        }
    }

    // Only provide hash rate when we have actual data
    // Don't simulate or estimate - be honest about what we can measure
    let realtime_estimate: Option<f64> = None; // No estimation - only real calculations

    // Only return hash rates based on actual measurements
    let final_hashrate = if let Some(calc_rate) = calculated_hashrate {
        // We have a real calculation from blocks found
        session_data.last_hash_rate = calc_rate;
        calc_rate
    } else if session_data.last_hash_rate > 0.0 {
        // Return the last measured hash rate from blocks
        session_data.last_hash_rate
    } else {
        // No blocks mined yet - can't provide hash rate
        0.0
    };

    // Store updated session data
    if let Ok(mut data) = MINING_SESSION_DATA.lock() {
        *data = session_data;
    }

    Ok((blocks_mined, final_hashrate))
}

pub fn get_mining_logs(data_dir: &str, lines: usize) -> Result<Vec<String>, String> {
    // Resolve relative data_dir against the executable directory
    let exe_dir = std::env::current_exe()
        .map_err(|e| format!("Failed to get exe path: {}", e))?
        .parent()
        .ok_or("Failed to get exe dir")?
        .to_path_buf();
    let data_path = if Path::new(data_dir).is_absolute() {
        PathBuf::from(data_dir)
    } else {
        exe_dir.join(data_dir)
    };
    let log_path = data_path.join("geth.log");

    if !log_path.exists() {
        return Ok(vec!["No logs available yet.".to_string()]);
    }

    let file = File::open(&log_path).map_err(|e| format!("Failed to open log file: {}", e))?;

    let reader = BufReader::new(file);
    let all_lines: Vec<String> = reader.lines().filter_map(Result::ok).collect();

    // Get the last N lines
    let start = if all_lines.len() > lines {
        all_lines.len() - lines
    } else {
        0
    };

    Ok(all_lines[start..].to_vec())
}

pub async fn get_mined_blocks_count(
    app: &tauri::AppHandle,
    miner_address: &str,
) -> Result<u64, String> {
    println!(
        "🔍 get_mined_blocks_count called for address: {}",
        miner_address
    );

    // Get the current block number
    let block_number_payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_blockNumber",
        "params": [],
        "id": 1
    });

    let response = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&block_number_payload)
        .send()
        .await
        .map_err(|e| format!("Failed to get block number: {}", e))?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    if let Some(error) = json_response.get("error") {
        return Err(format!("RPC error: {}", error));
    }

    let block_hex = json_response["result"]
        .as_str()
        .ok_or("Invalid block number response")?;

    let current_block = u64::from_str_radix(&block_hex[2..], 16)
        .map_err(|e| format!("Failed to parse block number: {}", e))?;

    // Normalize the miner address for comparison
    let normalized_miner = miner_address.to_lowercase();

    // Maintain cumulative count per address - only increases, never decreases
    static CUMULATIVE_COUNTS: Lazy<Mutex<HashMap<String, u64>>> =
        Lazy::new(|| Mutex::new(HashMap::new()));

    // Incremental scanning: only scan blocks we haven't checked yet
    // Much more efficient than rescanning the same blocks repeatedly
    static LAST_SCANNED_BLOCK: Lazy<Mutex<u64>> = Lazy::new(|| Mutex::new(0));

    // Start from the last scanned block, or very recent blocks if this is first scan
    let start_block = {
        let last_scanned = LAST_SCANNED_BLOCK.lock().await;
        if *last_scanned == 0 {
            // First scan: check minimal recent blocks since mining happens at tip
            // For active mining, our blocks are at the very end - minimal scanning needed
            current_block.saturating_sub(1)
        } else {
            // Incremental: start from where we left off
            *last_scanned
        }
    };

    // Get current cumulative count for this address (atomic with update)
    let current_cumulative = {
        let counts = CUMULATIVE_COUNTS.lock().await;
        *counts.get(miner_address).unwrap_or(&0)
    };

    // Update the last scanned block for next time
    {
        let mut last_scanned = LAST_SCANNED_BLOCK.lock().await;
        *last_scanned = current_block;
    }

    // Only scan recent blocks for efficiency

    // Process blocks one at a time for maximum incremental updates
    // Each block discovery triggers immediate UI feedback
    const BATCH_SIZE: usize = 1;
    let mut newly_discovered_blocks = 0u64;

    // Scan from newest to oldest for active mining (recent blocks more likely to be mined)
    // Process in smaller batches for more responsive incremental updates
    let mut scanned_blocks = 0u64;

    // Calculate batches from newest to oldest
    let mut batch_starts = Vec::new();
    let mut current_start = start_block;
    while current_start <= current_block {
        batch_starts.push(current_start);
        if current_start + BATCH_SIZE as u64 > current_block {
            break;
        }
        current_start += BATCH_SIZE as u64;
    }
    // Reverse to scan newest first
    batch_starts.reverse();

    // Process blocks sequentially for truly incremental discovery
    // Each block is checked individually with small delays between discoveries
    for block_num in start_block..=current_block {
        let block_payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_getBlockByNumber",
            "params": [format!("0x{:x}", block_num), false],
            "id": 1
        });

        // Check this block
        let mut block_result = 0u64;
        for attempt in 0..3 {
            if let Ok(response) = HTTP_CLIENT
                .post(&NETWORK_CONFIG.rpc_endpoint)
                .json(&block_payload)
                .send()
                .await
            {
                if let Ok(json_response) = response.json::<serde_json::Value>().await {
                    if let Some(block) = json_response.get("result") {
                        if let Some(block_miner) = block.get("miner").and_then(|m| m.as_str()) {
                            if block_miner.to_lowercase() == normalized_miner {
                                block_result = 1;
                                break;
                            }
                        }
                    }
                }
            }

            // Wait before retry
            if attempt < 2 {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        }

        scanned_blocks += 1;

        // If we found a mined block, update immediately
        if block_result > 0 {
            newly_discovered_blocks += block_result;

            // Update cumulative count immediately (drop guard before await)
            {
                let mut counts = CUMULATIVE_COUNTS.lock().await;
                let existing_count = *counts.get(miner_address).unwrap_or(&0);
                let new_total = existing_count + block_result;
                counts.insert(miner_address.to_string(), new_total);

                println!(
                    "🔍 Found 1 NEW block! (cumulative total: {}, scanned {} blocks)",
                    new_total, scanned_blocks
                );

                // Emit event to frontend for real-time UI updates
                let _ = app.emit(
                    "mining_scan_progress",
                    serde_json::json!({
                        "address": miner_address,
                        "blocks_found_in_batch": 1,
                        "total_scanned": scanned_blocks,
                        "timestamp": chrono::Utc::now().timestamp()
                    }),
                );
            }

            // Small delay between discoveries for better visual feedback
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        }
    }

    // Update the cumulative count with any newly discovered blocks
    let final_total = if newly_discovered_blocks > 0 {
        let mut counts = CUMULATIVE_COUNTS.lock().await;
        let existing_count = *counts.get(miner_address).unwrap_or(&0);
        let new_total = existing_count + newly_discovered_blocks;
        counts.insert(miner_address.to_string(), new_total);
        new_total
    } else {
        current_cumulative
    };

    // Return the cumulative count (existing + newly discovered)
    println!("🔍 get_mined_blocks_count returning: {} total blocks for address: {} (discovered {} new, previous total: {})",
             final_total, miner_address, newly_discovered_blocks, current_cumulative);
    Ok(final_total)
}

//Fetching Recent Blocks Mined by address, scanning backwards from latest
pub async fn get_recent_mined_blocks(
    miner_address: &str,
    lookback: u64,
    limit: usize,
) -> Result<Vec<MinedBlock>, String> {
    // Fetch latest block number
    let latest_v = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "params": [],
            "id": 1
        }))
        .send()
        .await
        .map_err(|e| format!("RPC send: {e}"))?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("RPC parse: {e}"))?;

    let latest_hex: &str = latest_v["result"]
        .as_str()
        .ok_or("Invalid eth_blockNumber")?;
    let latest = u64::from_str_radix(latest_hex.trim_start_matches("0x"), 16)
        .map_err(|e| format!("hex parse: {e}"))?;

    let start = latest.saturating_sub(lookback);
    let target = miner_address.to_lowercase();

    let mut out: Vec<MinedBlock> = Vec::new();

    for n in (start..=latest).rev() {
        if out.len() >= limit {
            break;
        }

        let block_v = HTTP_CLIENT
            .post(&NETWORK_CONFIG.rpc_endpoint)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "eth_getBlockByNumber",
                "params": [format!("0x{:x}", n), false],
                "id": 1
            }))
            .send()
            .await
            .map_err(|e| format!("RPC send: {e}"))?
            .json::<serde_json::Value>()
            .await
            .map_err(|e| format!("RPC parse: {e}"))?;

        if block_v.get("result").is_none() {
            continue;
        }
        let b = &block_v["result"];

        let miner = b
            .get("author")
            .and_then(|x| x.as_str())
            .or_else(|| b.get("miner").and_then(|x| x.as_str()))
            .unwrap_or("")
            .to_lowercase();

        if miner != target {
            continue;
        }

        let hash = b
            .get("hash")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string();
        let nonce = b
            .get("nonce")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        let difficulty = b
            .get("difficulty")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        let timestamp = b
            .get("timestamp")
            .and_then(|x| x.as_str())
            .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
            .unwrap_or(0);
        let number = b
            .get("number")
            .and_then(|x| x.as_str())
            .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
            .unwrap_or(n);

        // let reward = {
        //     // Balance at block n
        //     let bal_n_v = client
        //         .post("http://127.0.0.1:8545")
        //         .json(&serde_json::json!({
        //             "jsonrpc": "2.0",
        //             "method": "eth_getBalance",
        //             "params": [target, format!("0x{:x}", number)],
        //             "id": 1
        //         }))
        //         .send()
        //         .await
        //         .map_err(|e| format!("RPC send: {e}"))?
        //         .json::<serde_json::Value>()
        //         .await
        //         .map_err(|e| format!("RPC parse: {e}"))?;

        //     let bal_prev_v = client
        //         .post("http://127.0.0.1:8545")
        //         .json(&serde_json::json!({
        //             "jsonrpc": "2.0",
        //             "method": "eth_getBalance",
        //             "params": [target, format!("0x{:x}", number.saturating_sub(1))],
        //             "id": 1
        //         }))
        //         .send()
        //         .await
        //         .map_err(|e| format!("RPC send: {e}"))?
        //         .json::<serde_json::Value>()
        //         .await
        //         .map_err(|e| format!("RPC parse: {e}"))?;

        //     let parse_u128 = |hex_str: &str| -> Option<u128> {
        //         let s = hex_str.trim_start_matches("0x");
        //         u128::from_str_radix(s, 16).ok()
        //     };

        //     let bal_n = bal_n_v
        //         .get("result")
        //         .and_then(|v| v.as_str())
        //         .and_then(parse_u128);
        //     let bal_prev = bal_prev_v
        //         .get("result")
        //         .and_then(|v| v.as_str())
        //         .and_then(parse_u128);
        //     if let (Some(bn), Some(bp)) = (bal_n, bal_prev) {
        //         let delta_wei = bn.saturating_sub(bp);
        //         // Convert to ether-like units (divide by 1e18)
        //         let reward = (delta_wei as f64) / 1_000_000_000_000_000_000f64;
        //         Some(reward)
        //     } else {
        //         None
        //     }
        // };

        let reward = Some(BLOCK_REWARD);

        out.push(MinedBlock {
            hash,
            nonce,
            difficulty,
            timestamp,
            number,
            reward,
        });
    }

    Ok(out)
}

// Range-based mining blocks fetch (for progressive loading)
pub async fn get_mined_blocks_range(
    miner_address: &str,
    from_block: u64,
    to_block: u64,
) -> Result<Vec<MinedBlock>, String> {
    let target = miner_address.to_lowercase();
    let mut out: Vec<MinedBlock> = Vec::new();

    for n in (from_block..=to_block).rev() {
        let block_v = HTTP_CLIENT
            .post(&NETWORK_CONFIG.rpc_endpoint)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "eth_getBlockByNumber",
                "params": [format!("0x{:x}", n), false],
                "id": 1
            }))
            .send()
            .await
            .map_err(|e| format!("RPC send: {e}"))?
            .json::<serde_json::Value>()
            .await
            .map_err(|e| format!("RPC parse: {e}"))?;

        if block_v.get("result").is_none() {
            continue;
        }
        let b = &block_v["result"];

        let miner = b
            .get("author")
            .and_then(|x| x.as_str())
            .or_else(|| b.get("miner").and_then(|x| x.as_str()))
            .unwrap_or("")
            .to_lowercase();

        if miner != target {
            continue;
        }

        let hash = b
            .get("hash")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string();
        let nonce = b
            .get("nonce")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        let difficulty = b
            .get("difficulty")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        let timestamp = b
            .get("timestamp")
            .and_then(|x| x.as_str())
            .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
            .unwrap_or(0);
        let number = b
            .get("number")
            .and_then(|x| x.as_str())
            .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
            .unwrap_or(n);

        let reward = Some(BLOCK_REWARD);

        out.push(MinedBlock {
            hash,
            nonce,
            difficulty,
            timestamp,
            number,
            reward,
        });
    }

    Ok(out)
}

// Get total mining rewards by summing actual rewards from all mined blocks
// This is more accurate than blocksFound * 2 and returns just a number
pub async fn get_total_mining_rewards(miner_address: &str) -> Result<f64, String> {
    let target = miner_address.to_lowercase();
    let mut total_rewards = 0.0;

    // Get the current block number
    let current_block = get_block_number().await?;

    // Scan all blocks from 0 to current
    // This could be slow for many blocks, but it's a one-time calculation
    for n in 0..=current_block {
        let block_v = HTTP_CLIENT
            .post(&NETWORK_CONFIG.rpc_endpoint)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "eth_getBlockByNumber",
                "params": [format!("0x{:x}", n), false],
                "id": 1
            }))
            .send()
            .await
            .map_err(|e| format!("RPC send: {e}"))?
            .json::<serde_json::Value>()
            .await
            .map_err(|e| format!("RPC parse: {e}"))?;

        if block_v.get("result").is_none() {
            continue;
        }
        let b = &block_v["result"];

        let miner = b
            .get("author")
            .and_then(|x| x.as_str())
            .or_else(|| b.get("miner").and_then(|x| x.as_str()))
            .unwrap_or("")
            .to_lowercase();

        if miner == target {
            total_rewards += BLOCK_REWARD;
        }
    }

    Ok(total_rewards)
}

// Struct to return accurate totals from blockchain scan
#[derive(Debug, Serialize, Clone)]
pub struct AccurateTotals {
    pub blocks_mined: u64,
    pub total_received: f64,
    pub total_sent: f64,
}

// Progress event for accurate totals calculation
#[derive(Debug, Serialize, Clone)]
pub struct AccurateTotalsProgress {
    pub current_block: u64,
    pub total_blocks: u64,
    pub percentage: u8,
}

/// Scans the entire blockchain once and calculates:
/// - Total blocks mined by the address
/// - Total Chiral received (incoming transactions)
/// - Total Chiral sent (outgoing transactions)
/// Emits progress events via Tauri event system
pub async fn calculate_accurate_totals(
    address: &str,
    app_handle: tauri::AppHandle,
) -> Result<AccurateTotals, String> {
    let target_address = address.to_lowercase();
    let mut blocks_mined = 0u64;
    let mut total_received = 0.0;
    let mut total_sent = 0.0;

    // Get the current block number
    let current_block = get_block_number().await?;

    // Emit initial progress
    let _ = app_handle.emit(
        "accurate-totals-progress",
        AccurateTotalsProgress {
            current_block: 0,
            total_blocks: current_block,
            percentage: 0,
        },
    );

    // Scan ALL blocks from genesis for maximum accuracy
    for n in 0..=current_block {
        // Emit progress every 100 blocks
        if n % 100 == 0 {
            let percentage = ((n as f64 / current_block as f64) * 100.0) as u8;
            let _ = app_handle.emit(
                "accurate-totals-progress",
                AccurateTotalsProgress {
                    current_block: n,
                    total_blocks: current_block,
                    percentage,
                },
            );
        }

        // Get block with full transaction data
        let block_v = HTTP_CLIENT
            .post(&NETWORK_CONFIG.rpc_endpoint)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "eth_getBlockByNumber",
                "params": [format!("0x{:x}", n), true], // true = include full transaction objects
                "id": 1
            }))
            .send()
            .await
            .map_err(|e| format!("RPC send: {e}"))?
            .json::<serde_json::Value>()
            .await
            .map_err(|e| format!("RPC parse: {e}"))?;

        if block_v.get("result").is_none() {
            continue;
        }
        let b = &block_v["result"];

        // Check if this address mined this block
        let miner = b
            .get("author")
            .and_then(|x| x.as_str())
            .or_else(|| b.get("miner").and_then(|x| x.as_str()))
            .unwrap_or("")
            .to_lowercase();

        if miner == target_address {
            blocks_mined += 1;
        }

        // Process all transactions in this block
        if let Some(txs) = b.get("transactions").and_then(|t| t.as_array()) {
            for tx in txs {
                let from = tx
                    .get("from")
                    .and_then(|f| f.as_str())
                    .unwrap_or("")
                    .to_lowercase();
                let to = tx
                    .get("to")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_lowercase();
                let value_hex = tx.get("value").and_then(|v| v.as_str()).unwrap_or("0x0");

                // Parse value (Wei to Chiral: divide by 10^18)
                if let Ok(value_wei) = u128::from_str_radix(&value_hex.trim_start_matches("0x"), 16)
                {
                    let value_chiral = value_wei as f64 / 1e18;

                    // Check if this is a received transaction
                    if to == target_address && value_chiral > 0.0 {
                        total_received += value_chiral;
                    }

                    // Check if this is a sent transaction
                    if from == target_address && value_chiral > 0.0 {
                        total_sent += value_chiral;
                    }
                }
            }
        }
    }

    // Emit final progress (100%)
    let _ = app_handle.emit(
        "accurate-totals-progress",
        AccurateTotalsProgress {
            current_block: current_block,
            total_blocks: current_block,
            percentage: 100,
        },
    );

    Ok(AccurateTotals {
        blocks_mined,
        total_received,
        total_sent,
    })
}

#[tauri::command]
pub async fn get_network_hashrate() -> Result<String, String> {
    // First, try to get the actual network hashrate from eth_hashrate
    // This will return the sum of all miners that have submitted their hashrate
    let hashrate_payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_hashrate",
        "params": [],
        "id": 1
    });

    if let Ok(response) = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&hashrate_payload)
        .send()
        .await
    {
        if let Ok(json_response) = response.json::<serde_json::Value>().await {
            if json_response.get("error").is_none() {
                if let Some(hashrate_hex) = json_response["result"].as_str() {
                    // Parse the hashrate
                    let hex_str = if hashrate_hex.starts_with("0x") {
                        &hashrate_hex[2..]
                    } else {
                        hashrate_hex
                    };

                    if !hex_str.is_empty() && hex_str != "0" {
                        if let Ok(hashrate) = u64::from_str_radix(hex_str, 16) {
                            if hashrate > 0 {
                                // We have actual reported hashrate, use it
                                let formatted = if hashrate >= 1_000_000_000 {
                                    format!("{:.2} GH/s", hashrate as f64 / 1_000_000_000.0)
                                } else if hashrate >= 1_000_000 {
                                    format!("{:.2} MH/s", hashrate as f64 / 1_000_000.0)
                                } else if hashrate >= 1_000 {
                                    format!("{:.2} KH/s", hashrate as f64 / 1_000.0)
                                } else {
                                    format!("{} H/s", hashrate)
                                };
                                return Ok(formatted);
                            }
                        }
                    }
                }
            }
        }
    }

    // If eth_hashrate returns 0 or fails, estimate from difficulty
    // For private networks, get the latest two blocks to calculate actual block time
    let latest_block = json!({
        "jsonrpc": "2.0",
        "method": "eth_getBlockByNumber",
        "params": ["latest", true],
        "id": 1
    });

    let response = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&latest_block)
        .send()
        .await
        .map_err(|e| format!("Failed to get block: {}", e))?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    if let Some(error) = json_response.get("error") {
        return Err(format!("RPC error: {}", error));
    }

    let difficulty_hex = json_response["result"]["difficulty"]
        .as_str()
        .ok_or("Invalid difficulty response")?;

    // Convert hex to decimal
    let difficulty = u128::from_str_radix(&difficulty_hex[2..], 16)
        .map_err(|e| format!("Failed to parse difficulty: {}", e))?;

    // Get actual block time from recent blocks instead of using a hard-coded estimate
    let latest_block_number_hex = json_response["result"]["number"]
        .as_str()
        .ok_or("Invalid block number response")?;
    let latest_block_number = u64::from_str_radix(&latest_block_number_hex[2..], 16)
        .map_err(|e| format!("Failed to parse block number: {}", e))?;

    let latest_timestamp_hex = json_response["result"]["timestamp"]
        .as_str()
        .ok_or("Invalid timestamp response")?;
    let latest_timestamp = u64::from_str_radix(&latest_timestamp_hex[2..], 16)
        .map_err(|e| format!("Failed to parse timestamp: {}", e))?;

    // Calculate actual average block time from the last 100 blocks (or fewer if network is new)
    let lookback_blocks = 100.min(latest_block_number.saturating_sub(1));
    let mut actual_block_time = 15.0; // Default fallback

    if lookback_blocks > 0 {
        let previous_block_number = latest_block_number.saturating_sub(lookback_blocks);
        let previous_block = json!({
            "jsonrpc": "2.0",
            "method": "eth_getBlockByNumber",
            "params": [format!("0x{:x}", previous_block_number), false],
            "id": 1
        });

        if let Ok(prev_response) = HTTP_CLIENT
            .post(&NETWORK_CONFIG.rpc_endpoint)
            .json(&previous_block)
            .send()
            .await
        {
            if let Ok(prev_json) = prev_response.json::<serde_json::Value>().await {
                if let Some(prev_timestamp_hex) = prev_json["result"]["timestamp"].as_str() {
                    if let Ok(prev_timestamp) = u64::from_str_radix(&prev_timestamp_hex[2..], 16) {
                        let time_diff = latest_timestamp.saturating_sub(prev_timestamp);
                        if time_diff > 0 {
                            actual_block_time = time_diff as f64 / lookback_blocks as f64;
                        }
                    }
                }
            }
        }
    }

    // For Chiral private network, estimate network hashrate from difficulty and actual block time
    // Network Hashrate = Difficulty / Block Time
    // This gives us the hash rate needed to mine a block at this difficulty in the observed time
    let hashrate = difficulty as f64 / actual_block_time;

    // Convert to human-readable format
    let formatted = if hashrate >= 1_000_000_000_000.0 {
        format!("{:.2} TH/s", hashrate / 1_000_000_000_000.0)
    } else if hashrate >= 1_000_000_000.0 {
        format!("{:.2} GH/s", hashrate / 1_000_000_000.0)
    } else if hashrate >= 1_000_000.0 {
        format!("{:.2} MH/s", hashrate / 1_000_000.0)
    } else if hashrate >= 1_000.0 {
        format!("{:.2} KH/s", hashrate / 1_000.0)
    } else {
        format!("{:.0} H/s", hashrate)
    };

    Ok(formatted)
}

pub async fn send_transaction(
    from_address: &str,
    to_address: &str,
    amount_chiral: f64,
    private_key: &str,
) -> Result<String, String> {
    let private_key_clean = private_key.strip_prefix("0x").unwrap_or(private_key);

    let wallet: LocalWallet = private_key_clean
        .parse()
        .map_err(|e| format!("Invalid private key: {}", e))?;

    let wallet_address = format!("{:?}", wallet.address());
    if wallet_address.to_lowercase() != from_address.to_lowercase() {
        return Err(format!(
            "Private key doesn't match account. Expected: {}, Got: {}",
            from_address, wallet_address
        ));
    }

    // Debug network connectivity before sending
    tracing::info!("=== NETWORK DEBUG BEFORE SENDING ===");
    match get_peer_count().await {
        Ok(count) => {
            tracing::info!("   Peer count: {}", count);
            if count == 0 {
                tracing::warn!("   ⚠️ NO PEERS CONNECTED - transaction will not propagate!");
            }
        }
        Err(e) => tracing::error!("   Failed to get peer count: {}", e),
    }

    let provider =
        Provider::<Http>::try_from(NETWORK_CONFIG.rpc_endpoint.as_str()).map_err(|e| {
            format!(
                "Failed to connect to RPC ({}): {}",
                NETWORK_CONFIG.rpc_endpoint, e
            )
        })?;

    let wallet = wallet.with_chain_id(NETWORK_CONFIG.chain_id);

    let client = SignerMiddleware::new(provider.clone(), wallet);

    let to: Address = to_address
        .parse()
        .map_err(|e| format!("Invalid to address: {}", e))?;

    let amount_wei = U256::from((amount_chiral * 1_000_000_000_000_000_000.0) as u128);

    // Get nonce for pending block (includes pending transactions)
    let from_addr: Address = from_address
        .parse()
        .map_err(|e| format!("Invalid from address: {}", e))?;

    // Get both confirmed and pending nonces for debugging
    let confirmed_nonce = provider
        .get_transaction_count(from_addr, Some(BlockNumber::Latest.into()))
        .await
        .map_err(|e| format!("Failed to get confirmed nonce: {}", e))?;

    let nonce = provider
        .get_transaction_count(from_addr, Some(BlockNumber::Pending.into()))
        .await
        .map_err(|e| format!("Failed to get nonce: {}", e))?;

    tracing::info!(
        "   Confirmed nonce: {}, Pending nonce: {}",
        confirmed_nonce,
        nonce
    );
    if nonce > confirmed_nonce {
        tracing::warn!(
            "   ⚠️ There are {} pending transactions for this address!",
            nonce - confirmed_nonce
        );
    }

    // Get the actual gas price to be used in the transaction

    let base_fee = match provider.get_block(BlockNumber::Latest).await {
        Ok(Some(block)) => block.base_fee_per_gas.unwrap_or(U256::from(1)),
        _ => U256::from(1), // Fallback to minimal fee
    };

    // Set max fee to 2x base fee to handle fee fluctuations, priority fee to 1 wei
    let max_fee = base_fee * 2;
    let priority_fee = U256::from(1u64);
    let gas_limit = U256::from(21000u64);

    let gas_price = provider
        .get_gas_price()
        .await
        .map_err(|e| format!("Failed to get gas price: {}", e))?;
    let gas_cost = gas_price * gas_limit;
    let total_cost = amount_wei + gas_cost;

    // Check sender's balance

    let sender_balance = provider
        .get_balance(from_addr, None)
        .await
        .map_err(|e| format!("Failed to get sender balance: {}", e))?;
    tracing::info!("   Sender balance: {} wei", sender_balance);
    tracing::info!(
        "   Amount to send: {} wei, Gas cost: {} wei, Total needed: {} wei",
        amount_wei,
        gas_cost,
        total_cost
    );

    if sender_balance < total_cost {
        return Err(format!(
            "Insufficient balance. Have: {} wei, Need: {} wei (amount: {} + gas: {})",
            sender_balance, total_cost, amount_wei, gas_cost
        ));
    }
    tracing::info!(
        "   Base fee: {} wei, Max fee: {} wei, Priority fee: {} wei, Gas price: {} wei",
        base_fee,
        max_fee,
        priority_fee,
        gas_price
    );

    let tx = TransactionRequest::new()
        .to(to)
        .value(amount_wei)
        .gas(21000)
        .gas_price(gas_price)
        .nonce(nonce);

    let pending_tx = client
        .send_transaction(tx, None)
        .await
        .map_err(|e| format!("Failed to send transaction: {}", e))?;

    let tx_hash = format!("{:?}", pending_tx.tx_hash());

    tracing::info!(
        "✅ Transaction sent: {} from {} to {} amount {} CHIRAL",
        tx_hash,
        from_address,
        to_address,
        amount_chiral
    );
    tracing::info!(
        "   Nonce: {}, Gas Price: {} wei, Gas Limit: 21000, Chain ID: {}",
        nonce,
        gas_price,
        NETWORK_CONFIG.chain_id
    );

    // Verify the transaction was added to the local txpool
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Check txpool status and content
    if let Ok(status) = get_txpool_status().await {
        let pending_count = status
            .get("pending")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let queued_count = status.get("queued").and_then(|v| v.as_str()).unwrap_or("?");
        tracing::info!(
            "   TxPool Status: pending={}, queued={}",
            pending_count,
            queued_count
        );

        // If there are pending transactions, log their details
        if pending_count != "0x0" && pending_count != "?" {
            if let Ok(content) = get_txpool_content().await {
                if let Some(pending) = content.get("pending") {
                    tracing::info!("   TxPool PENDING content:");
                    if let Some(obj) = pending.as_object() {
                        for (addr, nonces) in obj {
                            tracing::info!("      Address: {}", addr);
                            if let Some(nonces_obj) = nonces.as_object() {
                                for (nonce, tx_data) in nonces_obj {
                                    let hash =
                                        tx_data.get("hash").and_then(|h| h.as_str()).unwrap_or("?");
                                    let to_addr =
                                        tx_data.get("to").and_then(|t| t.as_str()).unwrap_or("?");
                                    let value = tx_data
                                        .get("value")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("?");
                                    tracing::info!(
                                        "         Nonce {}: hash={}, to={}, value={}",
                                        nonce,
                                        hash,
                                        to_addr,
                                        value
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Log connected peers to verify network propagation
    match get_peer_info().await {
        Ok(peers) => {
            if let Some(peers_array) = peers.as_array() {
                if peers_array.is_empty() {
                    tracing::warn!("   ⚠️ NO PEERS - Transaction cannot propagate to other nodes!");
                } else {
                    tracing::info!(
                        "   Connected to {} peer(s) - transaction should propagate",
                        peers_array.len()
                    );
                    for peer in peers_array.iter().take(3) {
                        // Show first 3 peers
                        let remote_addr = peer
                            .get("network")
                            .and_then(|n| n.get("remoteAddress"))
                            .and_then(|a| a.as_str())
                            .unwrap_or("?");
                        tracing::info!("      Peer: {}", remote_addr);
                    }
                }
            }
        }
        Err(e) => tracing::warn!("   Could not get peer info: {}", e),
    }

    // Try to get the transaction back to verify it's in the pool
    match get_transaction_by_hash(tx_hash.clone()).await {
        Ok(Some(tx_data)) => {
            tracing::info!("✅ Transaction confirmed in local txpool: {}", tx_hash);
            // Log the blockNumber - if it's null, tx is still pending
            if let Some(block) = tx_data.get("blockNumber") {
                if block.is_null() {
                    tracing::info!("   Transaction is PENDING (not yet mined)");

                    // Spawn a background task to monitor the transaction
                    let tx_hash_clone = tx_hash.clone();
                    let from_clone = from_address.to_string();
                    let to_clone = to_address.to_string();
                    let amount_clone = amount_chiral;
                    tokio::spawn(async move {
                        tracing::info!("🔍 Monitoring transaction {} for mining...", tx_hash_clone);
                        for attempt in 1..=60 {
                            // Monitor for up to 60 seconds
                            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

                            match get_transaction_receipt(tx_hash_clone.clone()).await {
                                Ok(Some(receipt)) => {
                                    let block_num = receipt
                                        .get("blockNumber")
                                        .and_then(|b| b.as_str())
                                        .unwrap_or("?");
                                    let status = receipt
                                        .get("status")
                                        .and_then(|s| s.as_str())
                                        .unwrap_or("?");
                                    let gas_used = receipt
                                        .get("gasUsed")
                                        .and_then(|g| g.as_str())
                                        .unwrap_or("?");

                                    if status == "0x1" {
                                        tracing::info!("🎉 TRANSACTION MINED SUCCESSFULLY!");
                                        tracing::info!("   Hash: {}", tx_hash_clone);
                                        tracing::info!("   Block: {}", block_num);
                                        tracing::info!(
                                            "   From: {} -> To: {}",
                                            from_clone,
                                            to_clone
                                        );
                                        tracing::info!("   Amount: {} CHIRAL", amount_clone);
                                        tracing::info!("   Gas Used: {}", gas_used);
                                        tracing::info!("   Status: SUCCESS ✅");

                                        // Check balances after mining to verify transfer
                                        if let Ok(sender_balance) = get_balance(&from_clone).await {
                                            tracing::info!(
                                                "   Sender balance after: {} CHIRAL",
                                                sender_balance
                                            );
                                        }
                                        if let Ok(receiver_balance) = get_balance(&to_clone).await {
                                            tracing::info!(
                                                "   Receiver balance after: {} CHIRAL",
                                                receiver_balance
                                            );
                                        }
                                    } else {
                                        tracing::error!("❌ TRANSACTION MINED BUT FAILED!");
                                        tracing::error!("   Hash: {}", tx_hash_clone);
                                        tracing::error!("   Block: {}", block_num);
                                        tracing::error!(
                                            "   Status: {} (0x0 = failed, 0x1 = success)",
                                            status
                                        );
                                        tracing::error!("   Gas Used: {}", gas_used);
                                        tracing::error!("   Full receipt: {:?}", receipt);
                                    }
                                    return;
                                }
                                Ok(None) => {
                                    if attempt % 10 == 0 {
                                        tracing::info!(
                                            "   Still waiting for tx {} to be mined... ({}s)",
                                            tx_hash_clone,
                                            attempt
                                        );
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("   Error checking receipt: {}", e);
                                }
                            }
                        }
                        tracing::warn!(
                            "⚠️ Transaction {} still not mined after 60 seconds",
                            tx_hash_clone
                        );
                    });
                } else {
                    tracing::info!("   Transaction is in block: {}", block);
                    // Check receipt for success/failure
                    if let Ok(Some(receipt)) = get_transaction_receipt(tx_hash.clone()).await {
                        if let Some(status) = receipt.get("status") {
                            let status_str = status.as_str().unwrap_or("");
                            if status_str == "0x1" {
                                tracing::info!("   ✅ Transaction SUCCEEDED");
                            } else {
                                tracing::error!(
                                    "   ❌ Transaction FAILED (status: {})",
                                    status_str
                                );
                            }
                        }
                    }
                }
            }
        }
        Ok(None) => {
            tracing::warn!("⚠️  Transaction NOT found in local txpool: {}", tx_hash);
        }
        Err(e) => {
            tracing::error!("❌ Failed to verify transaction in txpool: {}", e);
        }
    }

    Ok(tx_hash)
}

/// Gets the transaction receipt to check if a transaction has been mined
pub async fn get_transaction_receipt(tx_hash: String) -> Result<Option<serde_json::Value>, String> {
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionReceipt",
        "params": [tx_hash],
        "id": 1
    });

    let response = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to get transaction receipt: {}", e))?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse receipt response: {}", e))?;

    if let Some(error) = json_response.get("error") {
        return Err(format!("RPC error: {}", error));
    }

    // If result is null, transaction hasn't been mined yet
    if json_response["result"].is_null() {
        return Ok(None);
    }

    Ok(Some(json_response["result"].clone()))
}

/// Gets transaction details by hash to check if it exists in the pool
#[tauri::command]
pub async fn get_transaction_by_hash(tx_hash: String) -> Result<Option<serde_json::Value>, String> {
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionByHash",
        "params": [tx_hash],
        "id": 1
    });

    let response = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to get transaction: {}", e))?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse transaction response: {}", e))?;

    if let Some(error) = json_response.get("error") {
        return Err(format!("RPC error: {}", error));
    }

    // If result is null, transaction doesn't exist
    if json_response["result"].is_null() {
        return Ok(None);
    }

    Ok(Some(json_response["result"].clone()))
}

/// Gets the pending transaction pool content to debug transaction issues
#[tauri::command]
pub async fn get_txpool_status() -> Result<serde_json::Value, String> {
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "txpool_status",
        "params": [],
        "id": 1
    });

    let response = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to get txpool status: {}", e))?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse txpool response: {}", e))?;

    if let Some(error) = json_response.get("error") {
        return Err(format!("RPC error: {}", error));
    }

    Ok(json_response["result"].clone())
}

/// Gets detailed pending transaction pool content for debugging
#[tauri::command]
pub async fn get_txpool_content() -> Result<serde_json::Value, String> {
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "txpool_content",
        "params": [],
        "id": 1
    });

    let response = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to get txpool content: {}", e))?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse txpool content response: {}", e))?;

    if let Some(error) = json_response.get("error") {
        return Err(format!("RPC error: {}", error));
    }

    Ok(json_response["result"].clone())
}

/// Fetches the full details of a block by its number.
/// This is used by the blockchain indexer to get reward data.
pub async fn get_block_details_by_number(
    block_number: u64,
) -> Result<Option<serde_json::Value>, String> {
    let client = reqwest::Client::new();
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getBlockByNumber",
        "params": [format!("0x{:x}", block_number), true], // true for full transaction objects
        "id": 1
    });

    let response = client
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to send request for block {}: {}", block_number, e))?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response for block {}: {}", block_number, e))?;

    if let Some(error) = json_response.get("error") {
        return Err(format!("RPC error for block {}: {}", block_number, error));
    }

    Ok(json_response["result"].clone().into())
}

/// Gets connected peer information for debugging network connectivity
#[tauri::command]
pub async fn get_peer_info() -> Result<serde_json::Value, String> {
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "admin_peers",
        "params": [],
        "id": 1
    });

    let response = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to get peer info: {}", e))?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse peer info response: {}", e))?;

    if let Some(error) = json_response.get("error") {
        return Err(format!("RPC error: {}", error));
    }

    let peers = &json_response["result"];
    if let Some(peers_array) = peers.as_array() {
        tracing::info!("Connected peers: {}", peers_array.len());
        for peer in peers_array {
            let name = peer
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown");
            let remote_addr = peer
                .get("network")
                .and_then(|n| n.get("remoteAddress"))
                .and_then(|a| a.as_str())
                .unwrap_or("?");
            tracing::info!("  Peer: {} at {}", name, remote_addr);
        }
    }

    Ok(json_response["result"].clone())
}

/// Debug function to check why transactions aren't being mined across network
#[tauri::command]
pub async fn debug_network_tx() -> Result<String, String> {
    let mut report = String::new();

    // 1. Check peer count
    match get_peer_count().await {
        Ok(count) => {
            report.push_str(&format!("Peer count: {}\n", count));
            if count == 0 {
                report.push_str("⚠️ WARNING: No peers connected! Transactions cannot propagate.\n");
            }
        }
        Err(e) => report.push_str(&format!("Failed to get peer count: {}\n", e)),
    }

    // 2. Check txpool
    match get_txpool_status().await {
        Ok(status) => {
            let pending = status
                .get("pending")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let queued = status.get("queued").and_then(|v| v.as_str()).unwrap_or("?");
            report.push_str(&format!("TxPool: pending={}, queued={}\n", pending, queued));
        }
        Err(e) => report.push_str(&format!("Failed to get txpool: {}\n", e)),
    }

    // 3. Check chain ID
    match get_chain_id().await {
        Ok(id) => report.push_str(&format!("Chain ID: {}\n", id)),
        Err(e) => report.push_str(&format!("Failed to get chain ID: {}\n", e)),
    }

    // 4. Get peer details
    match get_peer_info().await {
        Ok(peers) => {
            if let Some(peers_array) = peers.as_array() {
                report.push_str(&format!(
                    "Connected peers details ({}):\n",
                    peers_array.len()
                ));
                for peer in peers_array {
                    let name = peer
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("unknown");
                    let remote_addr = peer
                        .get("network")
                        .and_then(|n| n.get("remoteAddress"))
                        .and_then(|a| a.as_str())
                        .unwrap_or("?");
                    report.push_str(&format!("  - {} at {}\n", name, remote_addr));
                }
            }
        }
        Err(e) => report.push_str(&format!("Failed to get peer info: {}\n", e)),
    }

    tracing::info!("Network debug report:\n{}", report);
    Ok(report)
}

/// Gets the current coinbase (etherbase) address used for mining rewards
pub async fn get_coinbase() -> Result<String, String> {
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_coinbase",
        "params": [],
        "id": 1
    });

    let response = HTTP_CLIENT
        .post(&NETWORK_CONFIG.rpc_endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to get coinbase: {}", e))?;

    let json_response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse coinbase response: {}", e))?;

    if let Some(error) = json_response.get("error") {
        return Err(format!("RPC error: {}", error));
    }

    let coinbase = json_response["result"]
        .as_str()
        .ok_or("Invalid coinbase response")?;

    Ok(coinbase.to_string())
}

// ============================================================================
// Transaction History Scanning
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionHistoryItem {
    pub hash: String,
    pub from: String,
    pub to: Option<String>,
    pub value: String, // Wei as string
    pub block_number: u64,
    pub timestamp: u64,
    pub status: String,  // "success" or "failed"
    pub tx_type: String, // "sent" or "received"
    pub gas_used: Option<String>,
    pub gas_price: Option<String>,
}

/// Scan blocks for transactions involving a specific address
/// Returns transactions from newest to oldest
pub async fn get_transaction_history(
    address: &str,
    from_block: u64,
    to_block: u64,
) -> Result<Vec<TransactionHistoryItem>, String> {
    let client = reqwest::Client::new();
    let address_lower = address.to_lowercase();
    let mut transactions = Vec::new();

    // Scan blocks from newest to oldest
    for block_num in (from_block..=to_block).rev() {
        // Fetch block with full transaction objects
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_getBlockByNumber",
            "params": [format!("0x{:x}", block_num), true],
            "id": 1
        });

        let response = client
            .post(&NETWORK_CONFIG.rpc_endpoint)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch block {}: {}", block_num, e))?;

        let json_response: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse block {}: {}", block_num, e))?;

        if let Some(error) = json_response.get("error") {
            return Err(format!("RPC error for block {}: {}", block_num, error));
        }

        let block = match json_response["result"].as_object() {
            Some(b) => b,
            None => continue, // No block data
        };

        // Get block timestamp
        let timestamp = block
            .get("timestamp")
            .and_then(|t| t.as_str())
            .and_then(|t| u64::from_str_radix(t.trim_start_matches("0x"), 16).ok())
            .unwrap_or(0);

        // Check all transactions in this block
        if let Some(txs) = block.get("transactions").and_then(|t| t.as_array()) {
            for tx in txs {
                let tx_obj = match tx.as_object() {
                    Some(obj) => obj,
                    None => continue,
                };

                let tx_hash = tx_obj
                    .get("hash")
                    .and_then(|h| h.as_str())
                    .unwrap_or("")
                    .to_string();

                let from = tx_obj
                    .get("from")
                    .and_then(|f| f.as_str())
                    .unwrap_or("")
                    .to_lowercase();

                let to = tx_obj
                    .get("to")
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_lowercase());

                let value = tx_obj
                    .get("value")
                    .and_then(|v| v.as_str())
                    .unwrap_or("0x0")
                    .to_string();

                let gas_price = tx_obj
                    .get("gasPrice")
                    .and_then(|g| g.as_str())
                    .map(|s| s.to_string());

                // Check if this transaction involves our address
                let is_from_us = from == address_lower;
                let is_to_us = to.as_ref().map_or(false, |t| t == &address_lower);

                if !is_from_us && !is_to_us {
                    continue; // Skip transactions not involving our address
                }

                // Get transaction receipt to check status
                let receipt_payload = serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "eth_getTransactionReceipt",
                    "params": [tx_hash],
                    "id": 1
                });

                let receipt_response = client
                    .post(&NETWORK_CONFIG.rpc_endpoint)
                    .json(&receipt_payload)
                    .send()
                    .await
                    .map_err(|e| format!("Failed to fetch receipt: {}", e))?;

                let receipt_json: serde_json::Value = receipt_response
                    .json()
                    .await
                    .map_err(|e| format!("Failed to parse receipt: {}", e))?;

                let receipt = receipt_json["result"].as_object();

                let status = receipt
                    .and_then(|r| r.get("status"))
                    .and_then(|s| s.as_str())
                    .map(|s| if s == "0x1" { "success" } else { "failed" })
                    .unwrap_or("unknown")
                    .to_string();

                let gas_used = receipt
                    .and_then(|r| r.get("gasUsed"))
                    .and_then(|g| g.as_str())
                    .map(|s| s.to_string());

                // When sending to yourself, create both sent and received transactions
                if is_from_us && is_to_us {
                    // Create sent transaction
                    transactions.push(TransactionHistoryItem {
                        hash: tx_hash.clone(),
                        from: from.clone(),
                        to: to.clone(),
                        value: value.clone(),
                        block_number: block_num,
                        timestamp,
                        status: status.clone(),
                        tx_type: "sent".to_string(),
                        gas_used: gas_used.clone(),
                        gas_price: gas_price.clone(),
                    });
                    // Create received transaction
                    transactions.push(TransactionHistoryItem {
                        hash: tx_hash,
                        from: from.clone(),
                        to,
                        value,
                        block_number: block_num,
                        timestamp,
                        status,
                        tx_type: "received".to_string(),
                        gas_used,
                        gas_price,
                    });
                } else {
                    // Normal transaction to/from someone else
                    transactions.push(TransactionHistoryItem {
                        hash: tx_hash,
                        from: from.clone(),
                        to,
                        value,
                        block_number: block_num,
                        timestamp,
                        status,
                        tx_type: if is_from_us { "sent" } else { "received" }.to_string(),
                        gas_used,
                        gas_price,
                    });
                }
            }
        }
    }

    Ok(transactions)
}

/// Reset the incremental block scanning state
/// Call this when switching accounts or when cache is cleared
pub async fn reset_incremental_scanning() {
    static LAST_SCANNED_BLOCK: Lazy<Mutex<u64>> = Lazy::new(|| Mutex::new(0));
    let mut last_scanned = LAST_SCANNED_BLOCK.lock().await;
    *last_scanned = 0;

    // Also reset cumulative counts
    static CUMULATIVE_COUNTS: Lazy<Mutex<HashMap<String, u64>>> =
        Lazy::new(|| Mutex::new(HashMap::new()));
    let mut counts = CUMULATIVE_COUNTS.lock().await;
    counts.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::State as AxumState, routing::post, Json, Router};
    use once_cell::sync::Lazy;
    use serde_json::Value;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tokio::net::TcpListener;
    use tokio::sync::{oneshot, Mutex as AsyncMutex, Semaphore};

    static TEST_SEMAPHORE: Lazy<Arc<Semaphore>> = Lazy::new(|| Arc::new(Semaphore::new(1)));

    #[derive(Clone)]
    struct MockRpcState {
        peer_counts: Arc<AsyncMutex<VecDeque<Result<String, String>>>>,
        add_peer_results: Arc<AsyncMutex<VecDeque<Result<bool, String>>>>,
        add_peer_delay: Duration,
        net_peer_count_calls: Arc<AtomicUsize>,
        add_peer_calls: Arc<AtomicUsize>,
    }

    impl MockRpcState {
        fn new(
            peer_counts: Vec<Result<String, String>>,
            add_peer_results: Vec<Result<bool, String>>,
            add_peer_delay: Duration,
        ) -> Self {
            Self {
                peer_counts: Arc::new(AsyncMutex::new(peer_counts.into())),
                add_peer_results: Arc::new(AsyncMutex::new(add_peer_results.into())),
                add_peer_delay,
                net_peer_count_calls: Arc::new(AtomicUsize::new(0)),
                add_peer_calls: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn add_peer_call_count(&self) -> usize {
            self.add_peer_calls.load(Ordering::SeqCst)
        }
    }

    async fn mock_rpc_handler(
        AxumState(state): AxumState<MockRpcState>,
        Json(payload): Json<Value>,
    ) -> Json<Value> {
        let id = payload.get("id").cloned().unwrap_or_else(|| json!(1));
        let method = payload
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        match method {
            "net_peerCount" => {
                state.net_peer_count_calls.fetch_add(1, Ordering::SeqCst);
                let next = {
                    let mut queue = state.peer_counts.lock().await;
                    queue.pop_front().unwrap_or_else(|| Ok("0x0".to_string()))
                };
                match next {
                    Ok(hex_qty) => Json(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": hex_qty
                    })),
                    Err(msg) => Json(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32000, "message": msg }
                    })),
                }
            }
            "admin_addPeer" => {
                state.add_peer_calls.fetch_add(1, Ordering::SeqCst);
                if !state.add_peer_delay.is_zero() {
                    tokio::time::sleep(state.add_peer_delay).await;
                }
                let next = {
                    let mut queue = state.add_peer_results.lock().await;
                    queue.pop_front().unwrap_or(Ok(false))
                };
                match next {
                    Ok(v) => Json(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": v
                    })),
                    Err(msg) => Json(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32000, "message": msg }
                    })),
                }
            }
            _ => Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": "method not found" }
            })),
        }
    }

    async fn spawn_mock_rpc(state: MockRpcState) -> (String, oneshot::Sender<()>) {
        let app = Router::new()
            .route("/", post(mock_rpc_handler))
            .with_state(state);

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock rpc");
        let addr = listener.local_addr().expect("mock rpc addr");
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        tokio::spawn(async move {
            let server = axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                });
            let _ = server.await;
        });

        (format!("http://{}", addr), shutdown_tx)
    }

    async fn prime_stagnant_state(peer_count: u32, rpc_ready_ago: Duration) {
        let mut state = PEER_RECOVERY_STATE.lock().await;
        state.rpc_ready_at = Some(Instant::now().checked_sub(rpc_ready_ago).unwrap_or_else(Instant::now));
        state.cooldown_until = None;
        state.in_flight = false;
        state.attempt_idx = 0;
        state.seed_cursor = 0;
        state.sample_hist.clear();
        state.last_attempt = None;
        push_peer_sample(
            &mut state.sample_hist,
            peer_count,
            now_unix_ms().saturating_sub(30_000),
        );
    }

    #[test]
    fn parse_quantity_accepts_hex() {
        assert_eq!(parse_rpc_quantity_u32("0x0", "peer count").unwrap(), 0);
        assert_eq!(parse_rpc_quantity_u32("0xa", "peer count").unwrap(), 10);
        assert_eq!(parse_rpc_quantity_u32("0xA", "peer count").unwrap(), 10);
        assert_eq!(parse_rpc_quantity_u32("0xff", "peer count").unwrap(), 255);
    }

    #[test]
    fn parse_quantity_rejects_bad_format() {
        assert!(parse_rpc_quantity_u32("10", "peer count").is_err());
        assert!(parse_rpc_quantity_u32("0x", "peer count").is_err());
        assert!(parse_rpc_quantity_u32("0x00", "peer count").is_err());
        assert!(parse_rpc_quantity_u32("0xz", "peer count").is_err());
        assert!(parse_rpc_quantity_u32("0X10", "peer count").is_err());
        assert!(parse_rpc_quantity_u32("-0x1", "peer count").is_err());
        assert!(parse_rpc_quantity_u32(" 0x1", "peer count").is_err());
    }

    #[test]
    fn parse_seed_list_trims_and_dedups() {
        let parsed = parse_seed_enodes_list(
            " enode://abc@1.1.1.1:30303 ,enode://abc@1.1.1.1:30303,, enode://def@2.2.2.2:30303 ",
        )
        .unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0], "enode://abc@1.1.1.1:30303");
        assert_eq!(parsed[1], "enode://def@2.2.2.2:30303");
    }

    #[test]
    fn parse_seed_list_rejects_invalid() {
        let err = parse_seed_enodes_list("http://not-enode").unwrap_err();
        assert!(err.contains("must start with enode://"));
    }

    #[test]
    fn select_seed_batch_rotates_cursor() {
        let seeds = vec![
            "enode://a@1.1.1.1:30303".to_string(),
            "enode://b@2.2.2.2:30303".to_string(),
            "enode://c@3.3.3.3:30303".to_string(),
        ];
        let (round1, cursor1) = select_seed_batch(&seeds, 0, 2);
        assert_eq!(round1, vec![seeds[0].clone(), seeds[1].clone()]);
        let (round2, cursor2) = select_seed_batch(&seeds, cursor1, 2);
        assert_eq!(round2, vec![seeds[2].clone(), seeds[0].clone()]);
        let (round3, _) = select_seed_batch(&seeds, cursor2, 2);
        assert_eq!(round3, vec![seeds[1].clone(), seeds[2].clone()]);
    }

    #[test]
    fn stagnant_check_needs_span_and_no_growth() {
        let now = now_unix_ms();
        let mut samples = VecDeque::new();
        samples.push_back(PeerCountSample {
            peer_count: 2,
            timestamp_ms: now.saturating_sub(30_000),
        });
        samples.push_back(PeerCountSample {
            peer_count: 2,
            timestamp_ms: now,
        });
        assert!(is_stagnant(&samples, now, Duration::from_secs(20)));

        samples.push_back(PeerCountSample {
            peer_count: 3,
            timestamp_ms: now + 1,
        });
        assert!(!is_stagnant(&samples, now + 1, Duration::from_secs(20)));
    }

    #[test]
    fn eval_reason_follows_gate_order() {
        let mut inp = RecoveryEval {
            rpc_ok: true,
            window_active: true,
            healthy: false,
            saturated: false,
            in_flight: false,
            cooling_down: false,
            stagnant: false,
            attempts_exhausted: false,
        };
        assert_eq!(
            eval_recovery_reason(inp),
            PeerRecoveryReason::WaitingStagnation
        );

        inp.healthy = true;
        assert_eq!(eval_recovery_reason(inp), PeerRecoveryReason::Healthy);
        inp.healthy = false;
        inp.saturated = true;
        assert_eq!(eval_recovery_reason(inp), PeerRecoveryReason::Saturated);
        inp.saturated = false;
        inp.in_flight = true;
        assert_eq!(eval_recovery_reason(inp), PeerRecoveryReason::Attempting);
        inp.in_flight = false;
        inp.cooling_down = true;
        assert_eq!(eval_recovery_reason(inp), PeerRecoveryReason::CoolingDown);
        inp.cooling_down = false;
        inp.attempts_exhausted = true;
        assert_eq!(
            eval_recovery_reason(inp),
            PeerRecoveryReason::AttemptsExhausted
        );
        inp.attempts_exhausted = false;
        inp.rpc_ok = false;
        assert_eq!(eval_recovery_reason(inp), PeerRecoveryReason::RpcDown);
    }

    #[test]
    fn eval_reason_handles_window_expiry() {
        let inp = RecoveryEval {
            rpc_ok: true,
            window_active: false,
            healthy: false,
            saturated: false,
            in_flight: false,
            cooling_down: false,
            stagnant: true,
            attempts_exhausted: false,
        };
        assert_eq!(eval_recovery_reason(inp), PeerRecoveryReason::WindowExpired);
    }

    #[test]
    fn saturation_uses_headroom_threshold() {
        assert!(!is_saturated(94, 100, 5));
        assert!(is_saturated(95, 100, 5));
        assert!(is_saturated(100, 100, 5));
        assert!(is_saturated(0, 3, 5));
    }

    #[tokio::test]
    async fn recovery_rpc_down_skips_add_peer_attempts() {
        let _permit = TEST_SEMAPHORE
            .clone()
            .acquire_owned()
            .await
            .expect("test semaphore");
        reset_peer_recovery_state().await;

        let mock_state = MockRpcState::new(
            vec![Err("rpc unavailable".to_string())],
            vec![Ok(true)],
            Duration::from_millis(0),
        );
        let (endpoint, shutdown_tx) = spawn_mock_rpc(mock_state.clone()).await;

        let snapshot = reconnect_to_bootstrap_with_snapshot_inner(
            3,
            &endpoint,
            Some(vec!["enode://seed@127.0.0.1:30303".to_string()]),
        )
        .await
        .expect("snapshot should be returned");

        assert_eq!(snapshot.reason, PeerRecoveryReason::RpcDown);
        assert!(!snapshot.attempted);
        assert_eq!(mock_state.add_peer_call_count(), 0);

        let _ = shutdown_tx.send(());
    }

    #[tokio::test]
    async fn recovery_success_requires_peer_count_threshold_not_add_peer_result() {
        let _permit = TEST_SEMAPHORE
            .clone()
            .acquire_owned()
            .await
            .expect("test semaphore");
        reset_peer_recovery_state().await;
        prime_stagnant_state(1, Duration::from_secs(1)).await;

        let mock_state = MockRpcState::new(
            vec![Ok("0x1".to_string()), Ok("0x3".to_string())],
            vec![Ok(false)],
            Duration::from_millis(0),
        );
        let (endpoint, shutdown_tx) = spawn_mock_rpc(mock_state.clone()).await;

        let snapshot = reconnect_to_bootstrap_with_snapshot_inner(
            3,
            &endpoint,
            Some(vec!["enode://seed@127.0.0.1:30303".to_string()]),
        )
        .await
        .expect("snapshot should be returned");

        assert_eq!(snapshot.reason, PeerRecoveryReason::Healthy);
        assert!(snapshot.attempted);
        let last_attempt = snapshot.last_attempt.expect("last attempt present");
        assert_eq!(last_attempt.accepted_targets, 0);
        assert_eq!(last_attempt.attempted_targets, 1);
        assert_eq!(mock_state.add_peer_call_count(), 1);

        let _ = shutdown_tx.send(());
    }

    #[tokio::test]
    async fn recovery_window_expired_blocks_attempt_even_when_stagnant() {
        let _permit = TEST_SEMAPHORE
            .clone()
            .acquire_owned()
            .await
            .expect("test semaphore");
        reset_peer_recovery_state().await;
        prime_stagnant_state(1, Duration::from_secs(130)).await;

        let mock_state = MockRpcState::new(
            vec![Ok("0x1".to_string())],
            vec![Ok(true)],
            Duration::from_millis(0),
        );
        let (endpoint, shutdown_tx) = spawn_mock_rpc(mock_state.clone()).await;

        let snapshot = reconnect_to_bootstrap_with_snapshot_inner(
            3,
            &endpoint,
            Some(vec!["enode://seed@127.0.0.1:30303".to_string()]),
        )
        .await
        .expect("snapshot should be returned");

        assert_eq!(snapshot.reason, PeerRecoveryReason::WindowExpired);
        assert!(!snapshot.attempted);
        assert_eq!(mock_state.add_peer_call_count(), 0);

        let _ = shutdown_tx.send(());
    }

    #[tokio::test]
    async fn recovery_window_expired_stays_blocked_on_repeated_calls() {
        let _permit = TEST_SEMAPHORE
            .clone()
            .acquire_owned()
            .await
            .expect("test semaphore");
        reset_peer_recovery_state().await;
        prime_stagnant_state(1, Duration::from_secs(130)).await;

        let mock_state = MockRpcState::new(
            vec![Ok("0x1".to_string()), Ok("0x1".to_string())],
            vec![Ok(true)],
            Duration::from_millis(0),
        );
        let (endpoint, shutdown_tx) = spawn_mock_rpc(mock_state.clone()).await;
        let seed_targets = vec!["enode://seed@127.0.0.1:30303".to_string()];

        let first = reconnect_to_bootstrap_with_snapshot_inner(
            3,
            &endpoint,
            Some(seed_targets.clone()),
        )
        .await
        .expect("first snapshot");
        let second = reconnect_to_bootstrap_with_snapshot_inner(3, &endpoint, Some(seed_targets))
            .await
            .expect("second snapshot");

        assert_eq!(first.reason, PeerRecoveryReason::WindowExpired);
        assert_eq!(second.reason, PeerRecoveryReason::WindowExpired);
        assert!(!first.attempted);
        assert!(!second.attempted);
        assert_eq!(mock_state.add_peer_call_count(), 0);

        let _ = shutdown_tx.send(());
    }

    #[tokio::test]
    async fn recovery_saturated_blocks_attempts_even_when_stagnant() {
        let _permit = TEST_SEMAPHORE
            .clone()
            .acquire_owned()
            .await
            .expect("test semaphore");
        reset_peer_recovery_state().await;
        prime_stagnant_state(95, Duration::from_secs(30)).await;

        let mock_state = MockRpcState::new(
            vec![Ok("0x5f".to_string())],
            vec![Ok(true)],
            Duration::from_millis(0),
        );
        let (endpoint, shutdown_tx) = spawn_mock_rpc(mock_state.clone()).await;

        let snapshot = reconnect_to_bootstrap_with_snapshot_inner(
            120,
            &endpoint,
            Some(vec!["enode://seed@127.0.0.1:30303".to_string()]),
        )
        .await
        .expect("snapshot should be returned");

        assert_eq!(snapshot.reason, PeerRecoveryReason::Saturated);
        assert!(!snapshot.attempted);
        assert_eq!(mock_state.add_peer_call_count(), 0);

        let _ = shutdown_tx.send(());
    }

    #[tokio::test]
    async fn recovery_single_flight_prevents_duplicate_attempt_rounds() {
        let _permit = TEST_SEMAPHORE
            .clone()
            .acquire_owned()
            .await
            .expect("test semaphore");
        reset_peer_recovery_state().await;
        prime_stagnant_state(1, Duration::from_secs(1)).await;

        let mock_state = MockRpcState::new(
            vec![
                Ok("0x1".to_string()),
                Ok("0x1".to_string()),
                Ok("0x1".to_string()),
                Ok("0x1".to_string()),
            ],
            vec![Ok(true)],
            Duration::from_millis(300),
        );
        let (endpoint, shutdown_tx) = spawn_mock_rpc(mock_state.clone()).await;

        let endpoint_for_first = endpoint.clone();
        let first = tokio::spawn(async move {
            reconnect_to_bootstrap_with_snapshot_inner(
                3,
                &endpoint_for_first,
                Some(vec!["enode://seed@127.0.0.1:30303".to_string()]),
            )
            .await
            .expect("first snapshot")
        });

        tokio::time::sleep(Duration::from_millis(40)).await;

        let second = reconnect_to_bootstrap_with_snapshot_inner(
            3,
            &endpoint,
            Some(vec!["enode://seed@127.0.0.1:30303".to_string()]),
        )
        .await
        .expect("second snapshot");
        let first_snapshot = first.await.expect("join first");

        assert_eq!(second.reason, PeerRecoveryReason::Attempting);
        assert!(!second.attempted);
        assert!(first_snapshot.attempted);
        assert_eq!(mock_state.add_peer_call_count(), 1);

        let _ = shutdown_tx.send(());
    }

    #[tokio::test]
    async fn recovery_reset_cancels_inflight_state_mutation() {
        let _permit = TEST_SEMAPHORE
            .clone()
            .acquire_owned()
            .await
            .expect("test semaphore");
        reset_peer_recovery_state().await;
        prime_stagnant_state(1, Duration::from_secs(1)).await;

        let mock_state = MockRpcState::new(
            vec![Ok("0x1".to_string()), Ok("0x1".to_string())],
            vec![Ok(true)],
            Duration::from_millis(300),
        );
        let (endpoint, shutdown_tx) = spawn_mock_rpc(mock_state.clone()).await;

        let endpoint_for_attempt = endpoint.clone();
        let attempt = tokio::spawn(async move {
            reconnect_to_bootstrap_with_snapshot_inner(
                3,
                &endpoint_for_attempt,
                Some(vec!["enode://seed@127.0.0.1:30303".to_string()]),
            )
            .await
            .expect("snapshot")
        });

        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let in_flight = {
                    let state = PEER_RECOVERY_STATE.lock().await;
                    state.in_flight
                };
                if in_flight {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("recovery attempt should enter in_flight before reset");
        reset_peer_recovery_state().await;

        let snapshot = attempt.await.expect("join attempt");
        let state = PEER_RECOVERY_STATE.lock().await;

        assert_eq!(snapshot.reason, PeerRecoveryReason::WindowExpired);
        assert!(!state.in_flight);
        assert_eq!(state.attempt_idx, 0);
        assert!(state.cooldown_until.is_none());

        let _ = shutdown_tx.send(());
    }
}
