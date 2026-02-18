//! Geth Management Module for Chiral Network
//!
//! This module handles:
//! - Downloading Core-Geth binary
//! - Starting/stopping Geth process
//! - Genesis initialization
//! - RPC communication
//! - Bootstrap node health checking (via geth_bootstrap module)

use crate::geth_bootstrap;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// Configuration
// ============================================================================

/// Chiral Network chain ID
pub const CHAIN_ID: u64 = 98765;

/// Network ID (same as chain ID for our network)
pub const NETWORK_ID: u64 = 98765;

/// Tracks whether a local Geth process is running (set by GethProcess start/stop)
static LOCAL_GETH_RUNNING: AtomicBool = AtomicBool::new(false);

fn diagnostics_geth_log_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("chiral-network")
        .join("geth")
        .join("geth.log")
}

fn detect_log_level(message: &str) -> &'static str {
    let upper = message.to_ascii_uppercase();
    if upper.contains("ERROR") || upper.contains("FAILED") || message.contains("❌") {
        "ERROR"
    } else if upper.contains("WARN") || message.contains("⚠️") {
        "WARN"
    } else if upper.contains("DEBUG") || message.contains("🔍") {
        "DEBUG"
    } else {
        "INFO"
    }
}

fn detect_log_source(message: &str) -> &'static str {
    let lower = message.to_ascii_lowercase();
    if message.contains("⛏️") || lower.contains("mining") || lower.contains("hashrate") {
        "MINING"
    } else {
        "GETH"
    }
}

fn append_structured_geth_log(message: &str) {
    let log_path = diagnostics_geth_log_path();
    if let Some(parent) = log_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let level = detect_log_level(message);
    let source = detect_log_source(message);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&log_path) {
        for line in message.lines() {
            let entry = format!("[{}] [{}] [{}] {}\n", timestamp, level, source, line);
            let _ = file.write_all(entry.as_bytes());
        }
    }
}

macro_rules! println {
    ($($arg:tt)*) => {{
        append_structured_geth_log(&format!($($arg)*));
    }};
}

/// Shared RPC endpoint for balance, transaction, and state queries.
/// Always returns the remote bootstrap node so all clients see the same
/// canonical chain state.  Mining operations use `effective_rpc_endpoint()`
/// which routes to the local Geth when it is running.
/// Override with CHIRAL_RPC_ENDPOINT environment variable.
pub fn rpc_endpoint() -> String {
    std::env::var("CHIRAL_RPC_ENDPOINT")
        .unwrap_or_else(|_| "http://130.245.173.73:8545".to_string())
}

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: u64,
    pub percentage: f32,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GethStatus {
    pub installed: bool,
    pub running: bool,
    pub local_running: bool,
    pub syncing: bool,
    pub current_block: u64,
    pub highest_block: u64,
    pub peer_count: u32,
    pub chain_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MiningStatus {
    pub mining: bool,
    pub hash_rate: u64,
    pub miner_address: Option<String>,
    pub total_mined_wei: String,
    pub total_mined_chi: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinedBlock {
    pub block_number: u64,
    pub timestamp: u64,
    pub reward_wei: String,
    pub reward_chi: f64,
    pub difficulty: u64,
}

// ============================================================================
// Geth Downloader
// ============================================================================

pub struct GethDownloader {
    base_dir: PathBuf,
}

impl GethDownloader {
    pub fn new() -> Self {
        // Use executable's directory - bin/ lives next to the exe
        let base_dir = std::env::current_exe()
            .unwrap_or_else(|_| PathBuf::from("."))
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();

        GethDownloader { base_dir }
    }

    pub fn geth_path(&self) -> PathBuf {
        self.base_dir
            .join("bin")
            .join(if cfg!(target_os = "windows") {
                "geth.exe"
            } else {
                "geth"
            })
    }

    pub fn is_geth_installed(&self) -> bool {
        self.geth_path().exists()
    }

    fn get_download_url(&self) -> Result<String, String> {
        // Core-Geth v1.12.20 URLs for different platforms
        let url = match (std::env::consts::OS, std::env::consts::ARCH) {
            ("macos", "aarch64") | ("macos", "x86_64") => {
                "https://github.com/etclabscore/core-geth/releases/download/v1.12.20/core-geth-osx-v1.12.20.zip"
            }
            ("linux", "x86_64") => {
                "https://github.com/etclabscore/core-geth/releases/download/v1.12.20/core-geth-linux-v1.12.20.zip"
            }
            ("linux", "aarch64") => {
                "https://github.com/etclabscore/core-geth/releases/download/v1.12.20/core-geth-arm64-v1.12.20.zip"
            }
            ("windows", "x86_64") => {
                "https://github.com/etclabscore/core-geth/releases/download/v1.12.20/core-geth-win64-v1.12.20.zip"
            }
            _ => {
                return Err(format!(
                    "Unsupported platform: {} {}",
                    std::env::consts::OS,
                    std::env::consts::ARCH
                ))
            }
        };

        Ok(url.to_string())
    }

    pub async fn download_geth<F>(&self, progress_callback: F) -> Result<(), String>
    where
        F: Fn(DownloadProgress) + Send + 'static,
    {
        if self.is_geth_installed() {
            progress_callback(DownloadProgress {
                downloaded: 0,
                total: 0,
                percentage: 100.0,
                status: "Geth already installed".to_string(),
            });
            return Ok(());
        }

        let url = self.get_download_url()?;
        let bin_dir = self.base_dir.join("bin");

        // Create bin directory if it doesn't exist
        fs::create_dir_all(&bin_dir)
            .map_err(|e| format!("Failed to create bin directory: {}", e))?;

        progress_callback(DownloadProgress {
            downloaded: 0,
            total: 0,
            percentage: 0.0,
            status: "Starting download...".to_string(),
        });

        // Download the file
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to download from {}: {}", url, e))?;

        if !response.status().is_success() {
            return Err(format!("Download failed with status: {}", response.status()));
        }

        let total_size = response.content_length().unwrap_or(0);

        // Download with progress tracking
        let mut downloaded = 0u64;
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.map_err(|e| format!("Failed to read chunk: {}", e))?;
            downloaded += chunk.len() as u64;
            bytes.extend_from_slice(&chunk);

            let percentage = if total_size > 0 {
                (downloaded as f32 / total_size as f32) * 100.0
            } else {
                0.0
            };

            progress_callback(DownloadProgress {
                downloaded,
                total: total_size,
                percentage,
                status: format!("Downloading... {:.1} MB", downloaded as f32 / 1_048_576.0),
            });
        }

        progress_callback(DownloadProgress {
            downloaded: bytes.len() as u64,
            total: total_size,
            percentage: 100.0,
            status: "Download complete, extracting...".to_string(),
        });

        // Extract the zip file
        self.extract_zip(&bytes, &bin_dir)?;

        // Make the binary executable on Unix systems
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let geth_path = self.geth_path();
            if geth_path.exists() {
                let mut perms = fs::metadata(&geth_path)
                    .map_err(|e| format!("Failed to get geth metadata: {}", e))?
                    .permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&geth_path, perms)
                    .map_err(|e| format!("Failed to set geth permissions: {}", e))?;
            }
        }

        progress_callback(DownloadProgress {
            downloaded: total_size,
            total: total_size,
            percentage: 100.0,
            status: "Installation complete!".to_string(),
        });

        Ok(())
    }

    fn extract_zip(&self, data: &[u8], output_dir: &Path) -> Result<(), String> {
        let reader = Cursor::new(data);
        let mut archive =
            zip::ZipArchive::new(reader).map_err(|e| format!("Failed to read zip archive: {}", e))?;

        let geth_filename = if cfg!(target_os = "windows") {
            "geth.exe"
        } else {
            "geth"
        };

        for i in 0..archive.len() {
            let mut file = archive
                .by_index(i)
                .map_err(|e| format!("Failed to read zip entry: {}", e))?;

            if file.name().ends_with(geth_filename) {
                let geth_path = output_dir.join(geth_filename);
                let mut outfile = fs::File::create(&geth_path)
                    .map_err(|e| format!("Failed to create geth file: {}", e))?;
                std::io::copy(&mut file, &mut outfile)
                    .map_err(|e| format!("Failed to write geth file: {}", e))?;
                return Ok(());
            }
        }

        Err("Could not find geth binary in archive".to_string())
    }
}

// ============================================================================
// Geth Process Manager
// ============================================================================

pub struct GethProcess {
    child: Option<Child>,
    data_dir: PathBuf,
    downloader: GethDownloader,
    /// Last observed block number for hashrate estimation
    last_block: u64,
    /// Timestamp (seconds since epoch) when last_block was observed
    last_block_time: u64,
}

impl GethProcess {
    pub fn new() -> Self {
        let data_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("chiral-network")
            .join("geth");

        let process = GethProcess {
            child: None,
            data_dir,
            downloader: GethDownloader::new(),
            last_block: 0,
            last_block_time: 0,
        };

        // Kill any orphaned Geth from a previous session immediately on construction.
        // This runs at app startup before the user ever clicks "Start Node".
        process.kill_orphaned_geth();
        Self::remove_lock_files_recursive(&process.data_dir);

        process
    }

    /// Kill an orphaned Geth process using PID file + port-based fallback.
    /// Sends SIGTERM first for clean shutdown, escalates to SIGKILL if needed.
    /// Waits for confirmed process exit and resource cleanup before returning.
    fn kill_orphaned_geth(&self) {
        println!("🔍 kill_orphaned_geth() — checking for orphans...");
        let pid_path = self.data_dir.join("geth.pid");
        let ipc_path = self.data_dir.join("geth.ipc");

        // Collect PIDs to kill from multiple sources
        let mut pids_to_kill: Vec<u32> = Vec::new();

        // Source 1: PID file from previous session
        if let Ok(pid_str) = fs::read_to_string(&pid_path) {
            if let Ok(pid) = pid_str.trim().parse::<u32>() {
                println!("🔍 Found PID file with PID {}", pid);
                pids_to_kill.push(pid);
            }
            let _ = fs::remove_file(&pid_path);
        }

        // Source 2: fuser on port 8545 (catches cases where PID file was deleted
        // but Geth is still running, or a different Geth instance is on our port)
        if let Ok(output) = Command::new("fuser").args(["8545/tcp"]).stderr(Stdio::piped()).output() {
            // fuser outputs PIDs to stderr on some systems, stdout on others
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            for text in [stdout, stderr] {
                for token in text.split_whitespace() {
                    let cleaned = token.trim_end_matches(char::is_alphabetic);
                    if let Ok(pid) = cleaned.parse::<u32>() {
                        if !pids_to_kill.contains(&pid) {
                            println!("🔍 Found process on port 8545: PID {}", pid);
                            pids_to_kill.push(pid);
                        }
                    }
                }
            }
        }

        // Source 3: geth.ipc socket existence (another indicator)
        if ipc_path.exists() && pids_to_kill.is_empty() {
            println!("🔍 Found stale geth.ipc but no PID — will clean up after LOCK removal");
        }

        if pids_to_kill.is_empty() {
            // No orphans found, just clean up stale files
            if ipc_path.exists() {
                let _ = fs::remove_file(&ipc_path);
            }
            return;
        }

        // Kill each PID: SIGTERM first, then SIGKILL if needed
        for pid in &pids_to_kill {
            let pid_s = pid.to_string();
            let is_alive = || -> bool {
                Command::new("kill").args(["-0", &pid_s])
                    .output().map(|o| o.status.success()).unwrap_or(false)
            };

            if !is_alive() {
                continue;
            }

            println!("⚠️  Killing orphaned Geth (PID {}), sending SIGTERM", pid);
            let _ = Command::new("kill").arg(&pid_s).output();

            // Wait up to 5s for graceful exit
            let mut exited = false;
            for i in 0..10 {
                std::thread::sleep(std::time::Duration::from_millis(500));
                if !is_alive() {
                    println!("✅ Orphaned Geth (PID {}) exited after {}ms", pid, (i + 1) * 500);
                    exited = true;
                    break;
                }
            }

            if !exited {
                println!("⚠️  SIGTERM failed, sending SIGKILL to PID {}", pid);
                let _ = Command::new("kill").args(["-9", &pid_s]).output();
                for _ in 0..10 {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    if !is_alive() { break; }
                }
            }
        }

        // Wait for geth.ipc to disappear (confirms full resource release)
        if ipc_path.exists() {
            println!("⏳ Waiting for geth.ipc cleanup...");
            for _ in 0..10 {
                if !ipc_path.exists() { break; }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            if ipc_path.exists() {
                let _ = fs::remove_file(&ipc_path);
            }
        }
    }

    /// Recursively remove all files named "LOCK" under the given directory
    fn remove_lock_files_recursive(dir: &Path) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    Self::remove_lock_files_recursive(&path);
                } else if path.file_name().map(|n| n == "LOCK").unwrap_or(false) {
                    println!("⚠️  Removing stale LOCK file: {}", path.display());
                    let _ = fs::remove_file(&path);
                }
            }
        }
    }

    pub fn is_installed(&self) -> bool {
        self.downloader.is_geth_installed()
    }

    pub fn is_running(&self) -> bool {
        self.child.is_some()
    }

    pub fn geth_path(&self) -> PathBuf {
        self.downloader.geth_path()
    }

    /// Get the genesis.json content for Chiral Network.
    /// Must match V1 and the remote bootstrap node exactly so that
    /// all nodes produce the same genesis hash and can peer together.
    fn get_genesis_json() -> String {
        serde_json::json!({
            "config": {
                "chainId": CHAIN_ID,
                "homesteadBlock": 0,
                "eip150Block": 0,
                "eip155Block": 0,
                "eip158Block": 0,
                "byzantiumBlock": 0,
                "constantinopleBlock": 0,
                "petersburgBlock": 0,
                "istanbulBlock": 0,
                "berlinBlock": 0,
                "londonBlock": 0,
                "ethash": {}
            },
            "difficulty": "0xA00000000",
            "gasLimit": "0x47b760",
            "alloc": {},
            "coinbase": "0x0000000000000000000000000000000000000000",
            "extraData": "0x4b656570206f6e206b656570696e67206f6e21",
            "nonce": "0x0000000000000042",
            "mixhash": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "parentHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "timestamp": "0x68b3b2ca"
        }).to_string()
    }

    /// Initialize the blockchain with genesis
    fn init_genesis(&self) -> Result<(), String> {
        let geth_path = self.geth_path();

        // Create data directory
        fs::create_dir_all(&self.data_dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        // Write genesis.json
        let genesis_path = self.data_dir.join("genesis.json");
        fs::write(&genesis_path, Self::get_genesis_json())
            .map_err(|e| format!("Failed to write genesis.json: {}", e))?;

        // Initialize blockchain
        let output = Command::new(&geth_path)
            .arg("--datadir")
            .arg(&self.data_dir)
            .arg("init")
            .arg(&genesis_path)
            .output()
            .map_err(|e| format!("Failed to run geth init: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "Geth init failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        println!("✅ Blockchain initialized with chain ID {}", CHAIN_ID);
        Ok(())
    }

    /// Start Geth process
    pub async fn start(&mut self, miner_address: Option<&str>) -> Result<(), String> {
        if self.child.is_some() {
            return Err("Geth is already running".to_string());
        }

        if !self.is_installed() {
            return Err("Geth is not installed. Please download it first.".to_string());
        }

        println!("🚀 start() called — datadir: {}", self.data_dir.display());

        // Kill any orphaned Geth process from a previous app session.
        self.kill_orphaned_geth();

        // Remove ALL stale LOCK files from the data directory tree
        Self::remove_lock_files_recursive(&self.data_dir);

        // Debug: check what's on port 8545 right before spawning
        if let Ok(output) = Command::new("fuser").args(["8545/tcp"]).stderr(Stdio::piped()).output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stdout.trim().is_empty() || !stderr.trim().is_empty() {
                println!("⚠️  Port 8545 still in use! stdout='{}' stderr='{}'", stdout.trim(), stderr.trim());
            } else {
                println!("✅ Port 8545 is free");
            }
        }

        // Debug: check for LOCK files after cleanup
        if let Ok(entries) = fs::read_dir(self.data_dir.join("geth")) {
            for entry in entries.flatten() {
                if entry.file_name() == "LOCK" {
                    println!("⚠️  LOCK file still exists after cleanup: {}", entry.path().display());
                }
            }
        }
        // Also check chaindata LOCK
        let chaindata_lock = self.data_dir.join("geth").join("chaindata").join("LOCK");
        if chaindata_lock.exists() {
            println!("⚠️  chaindata/LOCK still exists after cleanup!");
        }
        // Check IPC
        let ipc_check = self.data_dir.join("geth.ipc");
        if ipc_check.exists() {
            println!("⚠️  geth.ipc still exists after cleanup!");
        }
        // Check PID file
        let pid_check = self.data_dir.join("geth.pid");
        if pid_check.exists() {
            println!("⚠️  geth.pid still exists after cleanup!");
        }

        // Check if blockchain needs initialization or re-initialization
        // Use a version marker to detect genesis config changes
        let genesis_version = "4"; // Bump this when genesis config changes
        let version_file = self.data_dir.join(".genesis_version");
        let chaindata_path = self.data_dir.join("geth").join("chaindata");
        let needs_init = if !chaindata_path.exists() {
            true
        } else {
            // Check if genesis version matches
            match fs::read_to_string(&version_file) {
                Ok(v) => v.trim() != genesis_version,
                Err(_) => true, // No version file = old genesis
            }
        };

        if needs_init {
            println!("Initializing blockchain (genesis v{})...", genesis_version);
            // Remove old chain data if it exists
            if chaindata_path.exists() {
                println!("Removing old chain data for genesis update...");
                let geth_dir = self.data_dir.join("geth");
                let _ = fs::remove_dir_all(&geth_dir);
            }
            self.init_genesis()?;
            fs::write(&version_file, genesis_version)
                .map_err(|e| format!("Failed to write genesis version: {}", e))?;
        }

        let geth_path = self.geth_path();

        // Get healthy bootstrap nodes (with health checking and caching)
        println!("🔍 Checking bootstrap node health...");
        let bootstrap_nodes = geth_bootstrap::get_healthy_enodes().await;
        println!("✅ Using bootstrap nodes: {}", if bootstrap_nodes.len() > 100 {
            format!("{}...", &bootstrap_nodes[..100])
        } else {
            bootstrap_nodes.clone()
        });

        let mut cmd = Command::new(&geth_path);
        cmd.arg("--datadir")
            .arg(&self.data_dir)
            .arg("--networkid")
            .arg(NETWORK_ID.to_string())
            .arg("--http")
            .arg("--http.addr")
            .arg("127.0.0.1")  // Only allow local RPC connections (security)
            .arg("--http.port")
            .arg("8545")
            .arg("--http.api")
            .arg("eth,net,web3,personal,debug,miner,admin,txpool")
            .arg("--http.corsdomain")
            .arg("*")
            .arg("--syncmode")
            .arg("full")  // Use full sync for local/private chain
            .arg("--gcmode")
            .arg("archive")  // Keep all state for querying
            .arg("--cache")
            .arg("512")
            .arg("--maxpeers")
            .arg("50")
            .arg("--port")
            .arg("30303")
            .arg("--nat")
            .arg("any")
            .arg("--miner.gasprice")
            .arg("0")
            .arg("--txpool.pricelimit")
            .arg("0")
            .arg("--metrics");

        // Add bootstrap nodes if available
        if !bootstrap_nodes.is_empty() {
            cmd.arg("--bootnodes").arg(&bootstrap_nodes);
        }

        // Set miner address and enable mining if provided
        if let Some(addr) = miner_address {
            cmd.arg("--miner.etherbase").arg(addr)
               .arg("--mine")
               .arg("--miner.threads").arg("1");
        }

        // Create log file (truncate on each start for clean logs)
        let log_path = self.data_dir.join("geth.log");
        let log_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&log_path)
            .map_err(|e| format!("Failed to create log file: {}", e))?;

        let log_file_clone = log_file
            .try_clone()
            .map_err(|e| format!("Failed to clone log file handle: {}", e))?;

        cmd.stdout(Stdio::from(log_file_clone))
            .stderr(Stdio::from(log_file));

        // Final cleanup right before spawn — remove LOCK files one more time
        // in case geth init or bootstrap check left any behind
        Self::remove_lock_files_recursive(&self.data_dir);

        println!("🚀 Spawning Geth...");
        let child = cmd
            .spawn()
            .map_err(|e| format!("Failed to start geth: {}", e))?;

        // Save the PID so we can kill the orphan on next start if the app crashes
        let pid = child.id();
        let pid_path = self.data_dir.join("geth.pid");
        let _ = fs::write(&pid_path, pid.to_string());
        println!("📝 Saved Geth PID {} to {}", pid, pid_path.display());

        self.child = Some(child);
        LOCAL_GETH_RUNNING.store(true, Ordering::Relaxed);

        println!("✅ Geth started");
        println!("   Logs: {}", log_path.display());
        println!("   RPC: http://127.0.0.1:8545");

        // Wait for Geth to start up
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        // Check if Geth crashed during startup
        if let Some(ref mut child) = self.child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.child = None;
                    LOCAL_GETH_RUNNING.store(false, Ordering::Relaxed);
                    // Read the log file for crash details
                    let log_contents = fs::read_to_string(&log_path).unwrap_or_default();
                    let last_lines: Vec<&str> = log_contents.lines().rev().take(30).collect();
                    let crash_log = last_lines.into_iter().rev().collect::<Vec<_>>().join("\n");
                    println!("❌ Geth crashed on startup (exit: {}):\n{}", status, crash_log);
                    return Err(format!("Geth crashed on startup (exit: {}). Check logs:\n{}", status, crash_log));
                }
                Ok(None) => {
                    println!("✅ Geth process still running after 3s startup wait");
                }
                Err(e) => {
                    self.child = None;
                    LOCAL_GETH_RUNNING.store(false, Ordering::Relaxed);
                    return Err(format!("Failed to check Geth process status: {}", e));
                }
            }
        }

        // Auto-start mining if miner address is set
        if miner_address.is_some() {
            println!("⛏️  Auto-starting mining...");
            match self.start_mining(1).await {
                Ok(_) => println!("✅ Mining started automatically"),
                Err(e) => println!("⚠️  Failed to auto-start mining: {}", e),
            }
        }

        Ok(())
    }

    /// Stop Geth process
    pub fn stop(&mut self) -> Result<(), String> {
        if let Some(mut child) = self.child.take() {
            LOCAL_GETH_RUNNING.store(false, Ordering::Relaxed);

            let pid = child.id();

            // Send SIGTERM first for graceful shutdown (releases LOCK files properly)
            let _ = Command::new("kill").arg(pid.to_string()).output();

            // Wait up to 5 seconds for graceful exit
            let mut exited = false;
            for _ in 0..10 {
                std::thread::sleep(std::time::Duration::from_millis(500));
                match child.try_wait() {
                    Ok(Some(_)) => { exited = true; break; }
                    Ok(None) => {} // still running
                    Err(_) => { exited = true; break; }
                }
            }

            // If still alive, force kill
            if !exited {
                let _ = child.kill();
                let _ = child.wait();
            }

            // Remove PID file
            let pid_path = self.data_dir.join("geth.pid");
            let _ = fs::remove_file(&pid_path);
            println!("✅ Geth stopped");
        }
        Ok(())
    }

    /// Get current Geth status via RPC
    pub async fn get_status(&mut self) -> Result<GethStatus, String> {
        // Check if the local Geth process has exited unexpectedly
        if let Some(ref mut child) = self.child {
            match child.try_wait() {
                Ok(Some(_status)) => {
                    // Process has exited, clean up
                    self.child = None;
                    LOCAL_GETH_RUNNING.store(false, Ordering::Relaxed);
                }
                Ok(None) => {} // Still running
                Err(_) => {
                    self.child = None;
                    LOCAL_GETH_RUNNING.store(false, Ordering::Relaxed);
                }
            }
        }

        let client = reqwest::Client::new();

        // Check if syncing
        let syncing_result = self.rpc_call(&client, "eth_syncing", serde_json::json!([])).await;
        let (syncing, current_block, highest_block) = match syncing_result {
            Ok(result) => {
                if result.is_boolean() && !result.as_bool().unwrap_or(false) {
                    (false, 0u64, 0u64)
                } else if let Some(obj) = result.as_object() {
                    let current = u64::from_str_radix(
                        obj.get("currentBlock")
                            .and_then(|v| v.as_str())
                            .unwrap_or("0x0")
                            .trim_start_matches("0x"),
                        16,
                    )
                    .unwrap_or(0);
                    let highest = u64::from_str_radix(
                        obj.get("highestBlock")
                            .and_then(|v| v.as_str())
                            .unwrap_or("0x0")
                            .trim_start_matches("0x"),
                        16,
                    )
                    .unwrap_or(0);
                    (true, current, highest)
                } else {
                    (false, 0, 0)
                }
            }
            Err(_) => (false, 0, 0),
        };

        // Get block number if not syncing
        let block_number = if !syncing {
            match self.rpc_call(&client, "eth_blockNumber", serde_json::json!([])).await {
                Ok(result) => {
                    let hex = result.as_str().unwrap_or("0x0");
                    u64::from_str_radix(hex.trim_start_matches("0x"), 16).unwrap_or(0)
                }
                Err(_) => 0,
            }
        } else {
            current_block
        };

        // Get peer count
        let peer_count = match self.rpc_call(&client, "net_peerCount", serde_json::json!([])).await {
            Ok(result) => {
                let hex = result.as_str().unwrap_or("0x0");
                u32::from_str_radix(hex.trim_start_matches("0x"), 16).unwrap_or(0)
            }
            Err(_) => 0,
        };

        // Get chain ID
        let chain_id = match self.rpc_call(&client, "eth_chainId", serde_json::json!([])).await {
            Ok(result) => {
                let hex = result.as_str().unwrap_or("0x0");
                u64::from_str_radix(hex.trim_start_matches("0x"), 16).unwrap_or(0)
            }
            Err(_) => 0,
        };

        Ok(GethStatus {
            installed: self.is_installed(),
            running: self.child.is_some(),
            local_running: self.child.is_some(),
            syncing,
            current_block: if syncing { current_block } else { block_number },
            highest_block: if syncing { highest_block } else { block_number },
            peer_count,
            chain_id,
        })
    }

    /// Start mining (requires local Geth process)
    pub async fn start_mining(&self, threads: u32) -> Result<(), String> {
        if self.child.is_none() {
            return Err("Cannot mine: local Geth node is not running. Start the node from the Network page first.".to_string());
        }
        let client = reqwest::Client::new();
        self.rpc_call(&client, "miner_start", serde_json::json!([threads]))
            .await
            .map(|_| ())
    }

    /// Stop mining
    pub async fn stop_mining(&self) -> Result<(), String> {
        let client = reqwest::Client::new();
        self.rpc_call(&client, "miner_stop", serde_json::json!([]))
            .await
            .map(|_| ())
    }

    /// Get mining status
    /// Note: eth_hashrate returns 0 for Geth's internal CPU miner (known upstream issue).
    /// We estimate hashrate from block difficulty / block time instead.
    pub async fn get_mining_status(&mut self) -> Result<MiningStatus, String> {
        let client = reqwest::Client::new();
        let rpc_url = self.effective_rpc_endpoint();

        println!("⛏️  ---- Mining Status Debug ----");
        println!("⛏️  RPC endpoint: {}", rpc_url);
        println!("⛏️  Local Geth running: {}", self.child.is_some());

        let mining = match self.rpc_call(&client, "eth_mining", serde_json::json!([])).await {
            Ok(result) => {
                let m = result.as_bool().unwrap_or(false);
                println!("⛏️  eth_mining: {} (raw: {})", m, result);
                m
            }
            Err(e) => {
                println!("⛏️  eth_mining: ERROR: {}", e);
                false
            }
        };

        // Estimate hashrate from block production:
        // hashrate ≈ difficulty / block_time
        let mut hash_rate: u64 = 0;

        if mining {
            // Get current block number
            let current_block = match self.rpc_call(&client, "eth_blockNumber", serde_json::json!([])).await {
                Ok(result) => {
                    let hex = result.as_str().unwrap_or("0x0");
                    let block = u64::from_str_radix(hex.trim_start_matches("0x"), 16).unwrap_or(0);
                    println!("⛏️  eth_blockNumber: {} (hex: {})", block, hex);
                    block
                }
                Err(e) => {
                    println!("⛏️  eth_blockNumber: ERROR: {}", e);
                    0
                }
            };

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            if current_block > 0 {
                // Get the latest block's difficulty
                let difficulty = match self.rpc_call(
                    &client,
                    "eth_getBlockByNumber",
                    serde_json::json!(["latest", false]),
                ).await {
                    Ok(result) => {
                        let diff = result.get("difficulty")
                            .and_then(|d| d.as_str())
                            .and_then(|hex| u64::from_str_radix(hex.trim_start_matches("0x"), 16).ok())
                            .unwrap_or(0);
                        let miner = result.get("miner").and_then(|m| m.as_str()).unwrap_or("unknown");
                        let block_num = result.get("number").and_then(|n| n.as_str()).unwrap_or("?");
                        println!("⛏️  Latest block: number={}, difficulty={}, miner={}", block_num, diff, miner);
                        diff
                    }
                    Err(e) => {
                        println!("⛏️  eth_getBlockByNumber: ERROR: {}", e);
                        0
                    }
                };

                println!("⛏️  Tracking: last_block={}, last_block_time={}, current_block={}, now={}",
                    self.last_block, self.last_block_time, current_block, now);

                if self.last_block > 0 && current_block > self.last_block && self.last_block_time > 0 {
                    // We have a previous measurement — compute hashrate from blocks mined
                    let blocks_mined = current_block - self.last_block;
                    let elapsed = now.saturating_sub(self.last_block_time);
                    if elapsed > 0 && difficulty > 0 {
                        // hashrate = (blocks_mined * difficulty) / elapsed_seconds
                        hash_rate = (blocks_mined as u128 * difficulty as u128 / elapsed as u128) as u64;
                    }
                    println!("⛏️  Blocks mined since last poll: {}, elapsed: {}s, hashrate: {}", blocks_mined, elapsed, hash_rate);
                } else if difficulty > 0 {
                    // First poll or no block change yet — estimate from difficulty alone
                    // Assume a ~13 second target block time as baseline
                    hash_rate = difficulty / 13;
                    println!("⛏️  First poll estimate: hashrate={} (difficulty/{} = {})", hash_rate, 13, difficulty);
                }

                // Update tracking
                self.last_block = current_block;
                self.last_block_time = now;
            } else {
                println!("⛏️  Block number is 0 — chain may still be initializing");
            }
        } else {
            // Not mining — reset tracking
            self.last_block = 0;
            self.last_block_time = 0;
            println!("⛏️  Mining is OFF");
        }

        let miner_address = match self.rpc_call(&client, "eth_coinbase", serde_json::json!([])).await
        {
            Ok(result) => {
                let addr = result.as_str().map(|s| s.to_string());
                println!("⛏️  eth_coinbase: {:?}", addr);
                addr
            }
            Err(e) => {
                println!("⛏️  eth_coinbase: ERROR: {}", e);
                None
            }
        };

        // Tail the Geth log file for recent activity
        let log_path = self.data_dir.join("geth.log");
        if log_path.exists() {
            match fs::read_to_string(&log_path) {
                Ok(contents) => {
                    let lines: Vec<&str> = contents.lines().collect();
                    let start = if lines.len() > 20 { lines.len() - 20 } else { 0 };
                    println!("⛏️  ---- Geth Log (last {} lines) ----", lines.len() - start);
                    for line in &lines[start..] {
                        println!("⛏️  LOG: {}", line);
                    }
                    println!("⛏️  ---- End Geth Log ----");
                }
                Err(e) => println!("⛏️  Could not read geth.log: {}", e),
            }
        } else {
            println!("⛏️  No geth.log found at {}", log_path.display());
        }

        // Query the miner's balance from the shared remote chain so it matches
        // the wallet balance shown on the Account page.
        let (total_mined_wei, total_mined_chi) = if let Some(ref addr) = miner_address {
            let balance_payload = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "eth_getBalance",
                "params": [addr, "latest"],
                "id": 1
            });
            match client.post(&rpc_endpoint())
                .json(&balance_payload)
                .send()
                .await
            {
                Ok(resp) => {
                    if let Ok(json) = resp.json::<serde_json::Value>().await {
                        let hex = json["result"].as_str().unwrap_or("0x0");
                        let wei = u128::from_str_radix(hex.trim_start_matches("0x"), 16).unwrap_or(0);
                        let chi = wei as f64 / 1e18;
                        println!("⛏️  Miner balance (remote): {} CHI ({} wei)", chi, wei);
                        (wei.to_string(), chi)
                    } else {
                        ("0".to_string(), 0.0)
                    }
                }
                Err(e) => {
                    println!("⛏️  eth_getBalance (remote): ERROR: {}", e);
                    ("0".to_string(), 0.0)
                }
            }
        } else {
            ("0".to_string(), 0.0)
        };

        println!("⛏️  ---- End Mining Status Debug ----");

        Ok(MiningStatus {
            mining,
            hash_rate,
            miner_address,
            total_mined_wei,
            total_mined_chi,
        })
    }

    /// Get blocks mined by the current miner address.
    /// Scans the last `max_blocks` blocks and returns those where the miner matches.
    pub async fn get_mined_blocks(&self, max_blocks: u64) -> Result<Vec<MinedBlock>, String> {
        let client = reqwest::Client::new();

        // Get current block number
        let current_block = match self.rpc_call(&client, "eth_blockNumber", serde_json::json!([])).await {
            Ok(result) => {
                let hex = result.as_str().unwrap_or("0x0");
                u64::from_str_radix(hex.trim_start_matches("0x"), 16).unwrap_or(0)
            }
            Err(_) => return Ok(Vec::new()),
        };

        if current_block == 0 {
            return Ok(Vec::new());
        }

        // Get miner address
        let miner_address = match self.rpc_call(&client, "eth_coinbase", serde_json::json!([])).await {
            Ok(result) => result.as_str().unwrap_or("").to_lowercase(),
            Err(_) => return Ok(Vec::new()),
        };

        if miner_address.is_empty() {
            return Ok(Vec::new());
        }

        let start_block = current_block.saturating_sub(max_blocks);
        let mut mined_blocks = Vec::new();

        // Scan blocks from newest to oldest
        for block_num in (start_block..=current_block).rev() {
            let hex_num = format!("0x{:x}", block_num);
            let block = match self.rpc_call(
                &client,
                "eth_getBlockByNumber",
                serde_json::json!([hex_num, false]),
            ).await {
                Ok(result) => result,
                Err(_) => continue,
            };

            let block_miner = block.get("miner")
                .and_then(|m| m.as_str())
                .unwrap_or("")
                .to_lowercase();

            if block_miner == miner_address {
                let timestamp = block.get("timestamp")
                    .and_then(|t| t.as_str())
                    .and_then(|hex| u64::from_str_radix(hex.trim_start_matches("0x"), 16).ok())
                    .unwrap_or(0);

                let difficulty = block.get("difficulty")
                    .and_then(|d| d.as_str())
                    .and_then(|hex| u64::from_str_radix(hex.trim_start_matches("0x"), 16).ok())
                    .unwrap_or(0);

                // Block reward is 5 ETH (5e18 wei) for ethash genesis configs
                let reward_wei: u128 = 5_000_000_000_000_000_000;
                let reward_chi = reward_wei as f64 / 1e18;

                mined_blocks.push(MinedBlock {
                    block_number: block_num,
                    timestamp,
                    reward_wei: reward_wei.to_string(),
                    reward_chi,
                    difficulty,
                });
            }

            // Cap results to avoid too much data
            if mined_blocks.len() >= 50 {
                break;
            }
        }

        Ok(mined_blocks)
    }

    /// Set miner address (coinbase)
    pub async fn set_miner_address(&self, address: &str) -> Result<(), String> {
        let client = reqwest::Client::new();
        self.rpc_call(&client, "miner_setEtherbase", serde_json::json!([address]))
            .await
            .map(|_| ())
    }

    /// Get the effective RPC endpoint: local Geth if running, otherwise shared remote
    pub fn effective_rpc_endpoint(&self) -> String {
        if self.child.is_some() {
            "http://127.0.0.1:8545".to_string()
        } else {
            rpc_endpoint()
        }
    }

    /// Make an RPC call to Geth
    async fn rpc_call(
        &self,
        _client: &reqwest::Client,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1
        });

        let endpoint = self.effective_rpc_endpoint();
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))?;
        let response = client
            .post(&endpoint)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("RPC request failed: {}", e))?;

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse RPC response: {}", e))?;

        if let Some(error) = json.get("error") {
            return Err(format!("RPC error: {}", error));
        }

        Ok(json["result"].clone())
    }
}

impl Drop for GethProcess {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chain_id_constant() {
        assert_eq!(CHAIN_ID, 98765);
    }

    #[test]
    fn test_network_id_matches_chain_id() {
        assert_eq!(NETWORK_ID, CHAIN_ID);
    }

    #[test]
    fn test_rpc_endpoint() {
        let endpoint = rpc_endpoint();
        assert!(endpoint.starts_with("http://"));
        assert!(endpoint.contains(":8545"));
    }

    #[test]
    fn test_rpc_endpoint_always_remote() {
        // rpc_endpoint() should always return remote, regardless of LOCAL_GETH_RUNNING
        LOCAL_GETH_RUNNING.store(true, Ordering::Relaxed);
        let endpoint = rpc_endpoint();
        if std::env::var("CHIRAL_RPC_ENDPOINT").is_err() {
            assert!(endpoint.contains("130.245.173.73"), "should be remote even when local flag is set");
        }
        // Reset
        LOCAL_GETH_RUNNING.store(false, Ordering::Relaxed);
    }

    #[test]
    fn test_genesis_json_is_valid() {
        let genesis = GethProcess::get_genesis_json();
        let parsed: serde_json::Value = serde_json::from_str(&genesis).unwrap();

        let config = parsed.get("config").expect("genesis should have config");
        let chain_id = config.get("chainId").expect("config should have chainId");
        assert_eq!(chain_id.as_u64().unwrap(), CHAIN_ID);

        assert_eq!(config["homesteadBlock"].as_u64().unwrap(), 0);
        assert_eq!(config["eip155Block"].as_u64().unwrap(), 0);
        assert_eq!(config["byzantiumBlock"].as_u64().unwrap(), 0);
        assert_eq!(config["petersburgBlock"].as_u64().unwrap(), 0);

        assert!(config.get("ethash").is_some());
        assert!(parsed.get("gasLimit").is_some());
        assert!(parsed.get("alloc").is_some());
    }

    #[test]
    fn test_genesis_has_empty_alloc() {
        let genesis = GethProcess::get_genesis_json();
        let parsed: serde_json::Value = serde_json::from_str(&genesis).unwrap();

        let alloc = parsed.get("alloc").expect("should have alloc");
        assert!(alloc.as_object().unwrap().is_empty(), "alloc should be empty to match bootstrap node");
    }

    #[test]
    fn test_genesis_difficulty_and_gas_limit() {
        let genesis = GethProcess::get_genesis_json();
        let parsed: serde_json::Value = serde_json::from_str(&genesis).unwrap();

        let difficulty = parsed.get("difficulty").unwrap().as_str().unwrap();
        assert!(difficulty.starts_with("0x"), "difficulty should be hex");
        let diff_val = u64::from_str_radix(difficulty.trim_start_matches("0x"), 16).unwrap();
        assert!(diff_val > 0, "difficulty should be non-zero");

        let gas_limit = parsed.get("gasLimit").unwrap().as_str().unwrap();
        assert!(gas_limit.starts_with("0x"), "gasLimit should be hex");
        let gas_val = u64::from_str_radix(gas_limit.trim_start_matches("0x"), 16).unwrap();
        assert!(gas_val > 1_000_000, "gasLimit should be at least 1M");

        let nonce = parsed.get("nonce").unwrap().as_str().unwrap();
        assert!(nonce.starts_with("0x"), "nonce should be hex");
    }

    #[test]
    fn test_genesis_extra_data() {
        let genesis = GethProcess::get_genesis_json();
        let parsed: serde_json::Value = serde_json::from_str(&genesis).unwrap();

        let extra_data = parsed.get("extraData").unwrap().as_str().unwrap();
        assert!(extra_data.starts_with("0x"));

        let bytes = hex::decode(extra_data.trim_start_matches("0x")).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert_eq!(text, "Keep on keeping on!");
    }

    #[test]
    fn test_download_progress_serialization() {
        let progress = DownloadProgress {
            downloaded: 1024,
            total: 4096,
            percentage: 25.0,
            status: "Downloading...".to_string(),
        };
        let json = serde_json::to_string(&progress).unwrap();
        let deserialized: DownloadProgress = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.downloaded, 1024);
        assert_eq!(deserialized.percentage, 25.0);
    }

    #[test]
    fn test_download_progress_uses_camel_case() {
        let progress = DownloadProgress {
            downloaded: 0,
            total: 100,
            percentage: 0.0,
            status: "test".to_string(),
        };
        let json = serde_json::to_string(&progress).unwrap();
        // serde rename_all = "camelCase" should not rename these (already lowercase)
        assert!(json.contains("\"downloaded\""));
        assert!(json.contains("\"total\""));
        assert!(json.contains("\"percentage\""));
        assert!(json.contains("\"status\""));
    }

    #[test]
    fn test_geth_status_serialization() {
        let status = GethStatus {
            installed: true,
            running: true,
            local_running: true,
            syncing: false,
            current_block: 100,
            highest_block: 100,
            peer_count: 5,
            chain_id: CHAIN_ID,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("currentBlock"));
        assert!(json.contains("peerCount"));
        assert!(json.contains("chainId"));
        assert!(json.contains("localRunning"));
        let deserialized: GethStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.chain_id, CHAIN_ID);
        assert!(deserialized.local_running);
    }

    #[test]
    fn test_geth_status_not_running() {
        let status = GethStatus {
            installed: false,
            running: false,
            local_running: false,
            syncing: false,
            current_block: 0,
            highest_block: 0,
            peer_count: 0,
            chain_id: 0,
        };
        let json = serde_json::to_string(&status).unwrap();
        let deserialized: GethStatus = serde_json::from_str(&json).unwrap();
        assert!(!deserialized.installed);
        assert!(!deserialized.running);
        assert_eq!(deserialized.chain_id, 0);
    }

    #[test]
    fn test_mining_status_serialization() {
        let status = MiningStatus {
            mining: true,
            hash_rate: 1000,
            miner_address: Some("0x1234567890abcdef".to_string()),
            total_mined_wei: "1000000000000000000".to_string(),
            total_mined_chi: 1.0,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("hashRate"));
        assert!(json.contains("minerAddress"));
        assert!(json.contains("totalMinedWei"));
        assert!(json.contains("totalMinedChi"));
        let deserialized: MiningStatus = serde_json::from_str(&json).unwrap();
        assert!(deserialized.mining);
        assert_eq!(deserialized.hash_rate, 1000);
    }

    #[test]
    fn test_mining_status_no_miner_address() {
        let status = MiningStatus {
            mining: false,
            hash_rate: 0,
            miner_address: None,
            total_mined_wei: "0".to_string(),
            total_mined_chi: 0.0,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"minerAddress\":null"));
        let deserialized: MiningStatus = serde_json::from_str(&json).unwrap();
        assert!(!deserialized.mining);
        assert!(deserialized.miner_address.is_none());
        assert_eq!(deserialized.total_mined_chi, 0.0);
    }

    #[test]
    fn test_mined_block_serialization() {
        let block = MinedBlock {
            block_number: 42,
            timestamp: 1700000000,
            reward_wei: "5000000000000000000".to_string(),
            reward_chi: 5.0,
            difficulty: 1024,
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("\"blockNumber\":42"));
        assert!(json.contains("\"timestamp\":1700000000"));
        assert!(json.contains("\"rewardWei\":\"5000000000000000000\""));
        assert!(json.contains("\"rewardChi\":5.0"));
        assert!(json.contains("\"difficulty\":1024"));
        let deserialized: MinedBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.block_number, 42);
        assert_eq!(deserialized.reward_chi, 5.0);
    }

    #[test]
    fn test_mined_block_uses_camel_case() {
        let block = MinedBlock {
            block_number: 1,
            timestamp: 0,
            reward_wei: "0".to_string(),
            reward_chi: 0.0,
            difficulty: 0,
        };
        let json = serde_json::to_string(&block).unwrap();
        // Verify camelCase (not snake_case)
        assert!(json.contains("blockNumber"));
        assert!(!json.contains("block_number"));
        assert!(json.contains("rewardWei"));
        assert!(!json.contains("reward_wei"));
        assert!(json.contains("rewardChi"));
        assert!(!json.contains("reward_chi"));
    }

    #[test]
    fn test_mined_block_deserialization_from_frontend_format() {
        // Frontend sends camelCase — verify we can deserialize it
        let json = r#"{"blockNumber":100,"timestamp":1700000000,"rewardWei":"5000000000000000000","rewardChi":5.0,"difficulty":512}"#;
        let block: MinedBlock = serde_json::from_str(json).unwrap();
        assert_eq!(block.block_number, 100);
        assert_eq!(block.timestamp, 1700000000);
        assert_eq!(block.reward_wei, "5000000000000000000");
        assert_eq!(block.reward_chi, 5.0);
        assert_eq!(block.difficulty, 512);
    }

    #[test]
    fn test_mined_block_vec_serialization() {
        let blocks = vec![
            MinedBlock {
                block_number: 10,
                timestamp: 1000,
                reward_wei: "5000000000000000000".to_string(),
                reward_chi: 5.0,
                difficulty: 256,
            },
            MinedBlock {
                block_number: 5,
                timestamp: 500,
                reward_wei: "5000000000000000000".to_string(),
                reward_chi: 5.0,
                difficulty: 128,
            },
        ];
        let json = serde_json::to_string(&blocks).unwrap();
        let deserialized: Vec<MinedBlock> = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.len(), 2);
        assert_eq!(deserialized[0].block_number, 10);
        assert_eq!(deserialized[1].block_number, 5);
    }

    #[test]
    fn test_mined_block_empty_vec_serialization() {
        let blocks: Vec<MinedBlock> = vec![];
        let json = serde_json::to_string(&blocks).unwrap();
        assert_eq!(json, "[]");
        let deserialized: Vec<MinedBlock> = serde_json::from_str(&json).unwrap();
        assert!(deserialized.is_empty());
    }

    #[test]
    fn test_remove_lock_files_in_temp_dir() {
        // Create a temp directory structure with LOCK files
        let tmp = std::env::temp_dir().join("chiral-test-lock-removal");
        let sub = tmp.join("chaindata");
        let _ = fs::create_dir_all(&sub);

        // Create LOCK files and a non-LOCK file
        let lock1 = tmp.join("LOCK");
        let lock2 = sub.join("LOCK");
        let keep = tmp.join("keep.txt");
        let _ = fs::write(&lock1, "");
        let _ = fs::write(&lock2, "");
        let _ = fs::write(&keep, "data");

        GethProcess::remove_lock_files_recursive(&tmp);

        assert!(!lock1.exists(), "LOCK in root should be removed");
        assert!(!lock2.exists(), "LOCK in subdir should be removed");
        assert!(keep.exists(), "Non-LOCK file should be kept");

        // Cleanup
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_remove_lock_files_nonexistent_dir() {
        // Should not panic on nonexistent directory
        let path = PathBuf::from("/nonexistent/path/for/testing");
        GethProcess::remove_lock_files_recursive(&path);
    }

    #[test]
    fn test_geth_process_data_dir_path() {
        // GethProcess::new() sets data_dir under the system data directory
        // We can't call new() (it runs kill_orphaned_geth) but we can verify the path pattern
        let data_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("chiral-network")
            .join("geth");
        assert!(data_dir.to_string_lossy().contains("chiral-network"));
        assert!(data_dir.to_string_lossy().ends_with("geth"));
    }

    #[test]
    fn test_geth_path_has_correct_extension() {
        let downloader = GethDownloader::new();
        let path = downloader.geth_path();
        let path_str = path.to_string_lossy();

        if cfg!(target_os = "windows") {
            assert!(path_str.ends_with("geth.exe"));
        } else {
            assert!(path_str.ends_with("geth"));
        }
    }

    #[test]
    fn test_geth_path_in_bin_directory() {
        let downloader = GethDownloader::new();
        let path = downloader.geth_path();
        let parent = path.parent().unwrap();
        assert_eq!(parent.file_name().unwrap().to_str().unwrap(), "bin");
    }
}
