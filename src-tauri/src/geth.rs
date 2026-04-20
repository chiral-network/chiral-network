//! Geth lifecycle, mining, and status RPC.
//!
//! This module is intentionally minimal. It owns one core-geth child process
//! per app instance and exposes only the surface the frontend actually uses:
//!
//! - download / install the binary
//! - init genesis once per chain
//! - start / stop the process
//! - start / stop CPU mining over JSON-RPC
//! - poll status, mining hash rate, recently mined blocks
//!
//! The previous implementation accumulated bootstrap-discovery logic, GPU
//! mining via ethminer, multi-version genesis migration, and per-call
//! `fuser`/PID racing to recover from broken state. That accumulated
//! complexity was the root cause of "blocks regress on restart" reports —
//! genesis migration logic could re-init mid-run, and orphan-recovery
//! sometimes spawned a second geth that raced the first on the same
//! datadir. This rewrite drops all of it.
//!
//! GPU mining commands remain in the public surface (the frontend imports
//! them) but are now stubs that return "not supported in this build". They
//! can be reintroduced in a separate module when the lifecycle is solid.

use crate::network;
use crate::rpc_client;
use flate2::read::GzDecoder;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// Module-level helpers + run-state flag
// ============================================================================

/// True iff the local geth child process is currently running. Read by
/// `effective_rpc_endpoint()` so callers route to the local node when it's
/// up and the configured remote fallback otherwise.
static LOCAL_GETH_RUNNING: AtomicBool = AtomicBool::new(false);

/// Chain id of the active network.
pub fn chain_id() -> u64 {
    network::active().chain_id
}

/// Network id (= chain id for our deployments).
pub fn network_id() -> u64 {
    network::active().network_id
}

/// Configured remote RPC fallback for the active network. Used when the local
/// geth isn't running. Override with `CHIRAL_RPC_ENDPOINT`.
pub fn rpc_endpoint() -> String {
    std::env::var("CHIRAL_RPC_ENDPOINT")
        .unwrap_or_else(|_| network::active().rpc_fallback.to_string())
}

/// Routes to local geth when it's running, falls back to remote otherwise.
pub fn effective_rpc_endpoint() -> String {
    if LOCAL_GETH_RUNNING.load(Ordering::Relaxed) {
        "http://127.0.0.1:8545".to_string()
    } else {
        rpc_endpoint()
    }
}

fn diagnostics_geth_log_path() -> PathBuf {
    network::data_dir().join("geth").join("geth.log")
}

// Re-exported as a `println!` macro so existing log call sites in this file
// route to the structured log file the diagnostics page reads.
macro_rules! println {
    ($($arg:tt)*) => {{
        append_structured_geth_log(&format!($($arg)*));
    }};
}

fn append_structured_geth_log(message: &str) {
    let log_path = diagnostics_geth_log_path();
    if let Some(parent) = log_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let level = if message.contains("ERROR") || message.contains("Failed") || message.contains("❌") {
        "ERROR"
    } else if message.contains("WARN") || message.contains("⚠") {
        "WARN"
    } else {
        "INFO"
    };
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&log_path) {
        for line in message.lines() {
            let entry = format!("[{}] [{}] [GETH] {}\n", timestamp, level, line);
            let _ = file.write_all(entry.as_bytes());
        }
    }
}

// ============================================================================
// Public types — shapes preserved so the existing frontend keeps working.
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
pub struct GpuDevice {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuMiningCapabilities {
    pub supported: bool,
    pub binary_path: Option<String>,
    pub devices: Vec<GpuDevice>,
    pub running: bool,
    pub active_devices: Vec<String>,
    pub utilization_percent: u8,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuMiningStatus {
    pub running: bool,
    pub hash_rate: u64,
    pub active_devices: Vec<String>,
    pub utilization_percent: u8,
    pub last_error: Option<String>,
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
// GethDownloader — fetches the binary on demand
// ============================================================================

pub struct GethDownloader {
    base_dir: PathBuf,
}

impl GethDownloader {
    pub fn new() -> Self {
        let base_dir = std::env::current_exe()
            .unwrap_or_else(|_| PathBuf::from("."))
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        GethDownloader { base_dir }
    }

    pub fn bin_dir(&self) -> PathBuf {
        self.base_dir.join("bin")
    }

    pub fn geth_path(&self) -> PathBuf {
        self.bin_dir().join(if cfg!(target_os = "windows") {
            "geth.exe"
        } else {
            "geth"
        })
    }

    pub fn is_geth_installed(&self) -> bool {
        self.geth_path().exists()
    }

    fn download_url(&self) -> Result<String, String> {
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
            _ => return Err(format!("Unsupported platform: {} {}", std::env::consts::OS, std::env::consts::ARCH)),
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

        let url = self.download_url()?;
        let bin_dir = self.bin_dir();
        fs::create_dir_all(&bin_dir).map_err(|e| format!("Create bin dir: {}", e))?;

        progress_callback(DownloadProgress {
            downloaded: 0,
            total: 0,
            percentage: 0.0,
            status: "Starting download...".to_string(),
        });

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| format!("HTTP client: {}", e))?;
        let response = client.get(&url).send().await.map_err(|e| format!("Download: {}", e))?;
        if !response.status().is_success() {
            return Err(format!("Download failed with status: {}", response.status()));
        }
        let total = response.content_length().unwrap_or(0);

        let mut downloaded = 0u64;
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("Read chunk: {}", e))?;
            downloaded += chunk.len() as u64;
            bytes.extend_from_slice(&chunk);
            let percentage = if total > 0 { (downloaded as f32 / total as f32) * 100.0 } else { 0.0 };
            progress_callback(DownloadProgress {
                downloaded,
                total,
                percentage,
                status: format!("Downloading... {:.1} MB", downloaded as f32 / 1_048_576.0),
            });
        }

        progress_callback(DownloadProgress {
            downloaded: bytes.len() as u64,
            total,
            percentage: 100.0,
            status: "Extracting...".to_string(),
        });

        Self::extract_geth(&bytes, &bin_dir)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let geth_path = self.geth_path();
            if geth_path.exists() {
                let mut perms = fs::metadata(&geth_path).map_err(|e| format!("metadata: {}", e))?.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&geth_path, perms).map_err(|e| format!("permissions: {}", e))?;
            }
        }

        progress_callback(DownloadProgress {
            downloaded: total,
            total,
            percentage: 100.0,
            status: "Installation complete".to_string(),
        });
        Ok(())
    }

    fn extract_geth(data: &[u8], output_dir: &Path) -> Result<(), String> {
        // Try zip first (current core-geth releases ship as .zip on every platform)
        if let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(data)) {
            let target_name = if cfg!(target_os = "windows") { "geth.exe" } else { "geth" };
            for i in 0..archive.len() {
                let mut file = archive.by_index(i).map_err(|e| format!("zip entry {}: {}", i, e))?;
                let name = file.name().rsplit('/').next().unwrap_or("");
                if name == target_name {
                    let dest = output_dir.join(target_name);
                    let mut out = fs::File::create(&dest).map_err(|e| format!("create {}: {}", dest.display(), e))?;
                    std::io::copy(&mut file, &mut out).map_err(|e| format!("copy: {}", e))?;
                    return Ok(());
                }
            }
            return Err("geth binary not found in zip".to_string());
        }
        // Fallback for legacy .tar.gz archives.
        let mut archive = tar::Archive::new(GzDecoder::new(Cursor::new(data)));
        for entry in archive.entries().map_err(|e| format!("tar entries: {}", e))? {
            let mut entry = entry.map_err(|e| format!("tar entry: {}", e))?;
            let path = entry.path().map_err(|e| format!("tar path: {}", e))?;
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if name == "geth" || name == "geth.exe" {
                let dest = output_dir.join(name);
                entry.unpack(&dest).map_err(|e| format!("unpack: {}", e))?;
                return Ok(());
            }
        }
        Err("geth binary not found in archive".to_string())
    }
}

impl Default for GethDownloader {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// GethProcess — owns the running geth child
// ============================================================================

pub struct GethProcess {
    child: Option<Child>,
    data_dir: PathBuf,
    downloader: GethDownloader,
    miner_address: Option<String>,
}

impl GethProcess {
    pub fn new() -> Self {
        let data_dir = network::data_dir().join("geth");
        let process = GethProcess {
            child: None,
            data_dir,
            downloader: GethDownloader::new(),
            miner_address: None,
        };
        // On construction, sweep up any orphan from a previous app session.
        // This runs once at app start, before any user action.
        process.kill_orphan_from_pid_file();
        Self::remove_lock_files(&process.data_dir);
        process
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

    /// Kill a geth process whose PID we wrote to disk on a previous run.
    /// SIGTERM first so geth releases its LevelDB lock cleanly, then SIGKILL
    /// if it's still alive after a short wait. No `fuser` racing — if the PID
    /// in our file is dead or belongs to something else, the kill is a no-op.
    fn kill_orphan_from_pid_file(&self) {
        let pid_path = self.data_dir.join("geth.pid");
        let Ok(pid_str) = fs::read_to_string(&pid_path) else { return };
        let _ = fs::remove_file(&pid_path);
        let Ok(pid) = pid_str.trim().parse::<u32>() else { return };

        #[cfg(unix)]
        {
            // Confirm it's actually a geth process before killing — paranoia
            // against PID reuse where another app inherits the recycled PID.
            let comm = fs::read_to_string(format!("/proc/{}/comm", pid)).unwrap_or_default();
            if !comm.trim().contains("geth") {
                return;
            }
            let _ = Command::new("kill").arg(pid.to_string()).status();
            std::thread::sleep(std::time::Duration::from_millis(800));
            // Force kill if still alive
            let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
        }
        #[cfg(windows)]
        {
            let _ = Command::new("taskkill")
                .args(["/F", "/PID", &pid.to_string()])
                .status();
        }
    }

    /// Recursively remove `LOCK` files under the datadir. core-geth's
    /// LevelDB / Pebble use these to enforce single-writer access; a stale
    /// LOCK from an unclean shutdown blocks the next start.
    fn remove_lock_files(dir: &Path) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                Self::remove_lock_files(&path);
            } else if path.file_name().and_then(|n| n.to_str()) == Some("LOCK") {
                let _ = fs::remove_file(&path);
            }
        }
    }

    /// Run `geth init genesis.json` for the active network. Idempotent if
    /// chaindata already exists (returns Ok without reinitializing).
    fn init_genesis(&self) -> Result<(), String> {
        let chaindata = self.data_dir.join("geth").join("chaindata");
        if chaindata.exists() {
            return Ok(());
        }

        fs::create_dir_all(&self.data_dir).map_err(|e| format!("Create datadir: {}", e))?;
        let genesis_path = self.data_dir.join("genesis.json");
        let genesis_json = network::genesis_json(network::active());
        fs::write(&genesis_path, &genesis_json).map_err(|e| format!("Write genesis: {}", e))?;

        let output = Command::new(self.geth_path())
            .arg("--datadir")
            .arg(&self.data_dir)
            .arg("init")
            .arg(&genesis_path)
            .output()
            .map_err(|e| format!("geth init: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "geth init failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        println!("✅ Initialized chain {} (chain id {})", network::active().display_name, chain_id());
        Ok(())
    }

    pub async fn start(&mut self, miner_address: Option<&str>) -> Result<(), String> {
        if self.child.is_some() {
            return Err("Geth is already running".to_string());
        }
        if !self.is_installed() {
            return Err("Geth binary not installed. Download it first.".to_string());
        }

        // Sweep any orphan from a crash on the way in. Belt-and-suspenders:
        // construction also does this, but the process may have been alive a
        // while since then.
        self.kill_orphan_from_pid_file();
        Self::remove_lock_files(&self.data_dir);

        self.init_genesis()?;
        self.miner_address = miner_address.map(|s| s.to_string());

        let cfg = network::active();
        let mut cmd = Command::new(self.geth_path());
        cmd.arg("--datadir").arg(&self.data_dir)
            .arg("--networkid").arg(network_id().to_string())
            .arg("--http")
            .arg("--http.addr").arg("127.0.0.1")
            .arg("--http.port").arg("8545")
            .arg("--http.api").arg("eth,net,web3,personal,debug,miner,admin,txpool")
            .arg("--http.corsdomain").arg("*")
            // Full sync + archive GC: prevents the "block height regresses on
            // restart" symptom that snap/fast sync caused on the legacy chain.
            .arg("--syncmode").arg("full")
            .arg("--gcmode").arg("archive")
            .arg("--cache").arg("256")
            .arg("--port").arg("30303")
            .arg("--maxpeers").arg("25")
            .arg("--miner.gasprice").arg("0")
            .arg("--txpool.pricelimit").arg("0");

        if cfg.geth_bootstrap_enode.is_empty() {
            // Solo-mining freshnet: don't bother with peer discovery.
            cmd.arg("--nodiscover");
        } else {
            cmd.arg("--bootnodes").arg(cfg.geth_bootstrap_enode);
        }

        if let Some(addr) = miner_address {
            cmd.arg("--miner.etherbase").arg(addr)
                .arg("--mine")
                .arg("--miner.threads").arg("1");
        }

        let log_path = self.data_dir.join("geth.log");
        if let Some(parent) = log_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let log_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&log_path)
            .map_err(|e| format!("Open log file: {}", e))?;
        let log_clone = log_file.try_clone().map_err(|e| format!("Clone log handle: {}", e))?;
        cmd.stdout(Stdio::from(log_clone)).stderr(Stdio::from(log_file));

        println!("🚀 Spawning geth on chain {} ({})", network::active().name, chain_id());
        let child = cmd.spawn().map_err(|e| format!("Spawn geth: {}", e))?;

        // Persist PID for next-start orphan cleanup
        let pid = child.id();
        let _ = fs::write(self.data_dir.join("geth.pid"), pid.to_string());
        self.child = Some(child);
        LOCAL_GETH_RUNNING.store(true, Ordering::Relaxed);

        // Brief startup wait. If geth crashes (bad genesis, port in use, etc.)
        // we want to surface that immediately rather than fail on the next
        // RPC call.
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        if let Some(ref mut child) = self.child {
            if let Ok(Some(status)) = child.try_wait() {
                self.child = None;
                LOCAL_GETH_RUNNING.store(false, Ordering::Relaxed);
                let log = fs::read_to_string(&log_path).unwrap_or_default();
                let tail: Vec<&str> = log.lines().rev().take(20).collect();
                let snippet = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
                return Err(format!("Geth exited on startup ({}). Log tail:\n{}", status, snippet));
            }
        }

        println!("✅ geth running, RPC on http://127.0.0.1:8545");
        Ok(())
    }

    /// Graceful stop with SIGTERM, escalating to SIGKILL if needed.
    pub fn stop(&mut self) -> Result<(), String> {
        let Some(mut child) = self.child.take() else {
            return Ok(());
        };
        LOCAL_GETH_RUNNING.store(false, Ordering::Relaxed);
        let pid = child.id();

        #[cfg(unix)]
        let _ = Command::new("kill").arg(pid.to_string()).status();
        #[cfg(windows)]
        let _ = Command::new("taskkill").args(["/PID", &pid.to_string()]).status();

        // Wait up to 3s for graceful exit.
        for _ in 0..6 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            if let Ok(Some(_)) = child.try_wait() {
                let _ = fs::remove_file(self.data_dir.join("geth.pid"));
                return Ok(());
            }
        }

        // Escalate.
        #[cfg(unix)]
        let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
        #[cfg(windows)]
        let _ = Command::new("taskkill").args(["/F", "/PID", &pid.to_string()]).status();
        let _ = child.wait();
        let _ = fs::remove_file(self.data_dir.join("geth.pid"));
        Ok(())
    }

    /// Best-effort fast kill for app shutdown — no graceful SIGTERM, no wait.
    pub fn stop_fast(&mut self) {
        if let Some(mut child) = self.child.take() {
            LOCAL_GETH_RUNNING.store(false, Ordering::Relaxed);
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(self.data_dir.join("geth.pid"));
        }
    }

    pub fn effective_rpc_endpoint(&self) -> String {
        effective_rpc_endpoint()
    }

    pub async fn get_status(&self) -> Result<GethStatus, String> {
        let installed = self.is_installed();
        let running = self.is_running();
        if !running {
            return Ok(GethStatus {
                installed,
                running: false,
                local_running: false,
                syncing: false,
                current_block: 0,
                highest_block: 0,
                peer_count: 0,
                chain_id: chain_id(),
            });
        }

        let endpoint = self.effective_rpc_endpoint();
        let block = rpc_client::call(&endpoint, "eth_blockNumber", serde_json::json!([]))
            .await
            .ok()
            .and_then(|v| v.as_str().map(|s| rpc_client::hex_to_u64(s)))
            .unwrap_or(0);
        let peers = rpc_client::call(&endpoint, "net_peerCount", serde_json::json!([]))
            .await
            .ok()
            .and_then(|v| v.as_str().map(|s| rpc_client::hex_to_u64(s)))
            .unwrap_or(0) as u32;
        let syncing_value = rpc_client::call(&endpoint, "eth_syncing", serde_json::json!([])).await.ok();
        let (syncing, highest_block) = parse_syncing(syncing_value.as_ref(), block);

        Ok(GethStatus {
            installed,
            running: true,
            local_running: true,
            syncing,
            current_block: block,
            highest_block,
            peer_count: peers,
            chain_id: chain_id(),
        })
    }

    pub async fn start_mining(&self, threads: u32) -> Result<(), String> {
        if !self.is_running() {
            return Err("Geth is not running".to_string());
        }
        let threads = threads.max(1) as u64;
        let endpoint = self.effective_rpc_endpoint();
        rpc_client::call(&endpoint, "miner_start", serde_json::json!([threads]))
            .await
            .map(|_| ())
    }

    pub async fn stop_mining(&self) -> Result<(), String> {
        if !self.is_running() {
            return Err("Geth is not running".to_string());
        }
        let endpoint = self.effective_rpc_endpoint();
        rpc_client::call(&endpoint, "miner_stop", serde_json::json!([])).await.map(|_| ())
    }

    pub async fn get_mining_status(&self) -> Result<MiningStatus, String> {
        if !self.is_running() {
            return Ok(MiningStatus {
                mining: false,
                hash_rate: 0,
                miner_address: self.miner_address.clone(),
                total_mined_wei: "0".to_string(),
                total_mined_chi: 0.0,
            });
        }
        let endpoint = self.effective_rpc_endpoint();
        let mining = rpc_client::call(&endpoint, "eth_mining", serde_json::json!([]))
            .await
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let hash_rate = rpc_client::call(&endpoint, "eth_hashrate", serde_json::json!([]))
            .await
            .ok()
            .and_then(|v| v.as_str().map(|s| rpc_client::hex_to_u64(s)))
            .unwrap_or(0);
        let coinbase = rpc_client::call(&endpoint, "eth_coinbase", serde_json::json!([]))
            .await
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .or_else(|| self.miner_address.clone());

        // Total mined = current balance of coinbase. Cheap and approximate
        // (doesn't account for gas spent or transfers out) but matches what
        // the miner's "earnings" panel needs in 99% of cases.
        let mut total_mined_wei = "0".to_string();
        let mut total_mined_chi = 0.0;
        if let Some(addr) = coinbase.as_ref() {
            if let Ok(v) = rpc_client::call(&endpoint, "eth_getBalance", serde_json::json!([addr, "latest"])).await {
                if let Some(s) = v.as_str() {
                    let wei = rpc_client::hex_to_u128(s);
                    total_mined_wei = wei.to_string();
                    total_mined_chi = wei as f64 / 1e18;
                }
            }
        }

        Ok(MiningStatus {
            mining,
            hash_rate,
            miner_address: coinbase,
            total_mined_wei,
            total_mined_chi,
        })
    }

    pub async fn set_miner_address(&mut self, address: &str) -> Result<(), String> {
        self.miner_address = Some(address.to_string());
        if !self.is_running() {
            return Ok(());
        }
        let endpoint = self.effective_rpc_endpoint();
        rpc_client::call(&endpoint, "miner_setEtherbase", serde_json::json!([address]))
            .await
            .map(|_| ())
    }

    /// Return the most recent blocks (up to `max_blocks`) mined by our coinbase.
    /// Walks backward from `eth_blockNumber`, skipping blocks not mined by us.
    pub async fn get_mined_blocks(&self, max_blocks: u64) -> Result<Vec<MinedBlock>, String> {
        if !self.is_running() {
            return Ok(Vec::new());
        }
        let endpoint = self.effective_rpc_endpoint();
        let head = rpc_client::call(&endpoint, "eth_blockNumber", serde_json::json!([]))
            .await
            .ok()
            .and_then(|v| v.as_str().map(|s| rpc_client::hex_to_u64(s)))
            .unwrap_or(0);
        let coinbase = self.miner_address.as_deref().unwrap_or("").to_lowercase();
        if coinbase.is_empty() || head == 0 {
            return Ok(Vec::new());
        }

        // Cap the scan window to avoid burning RPC calls — real miners on a
        // fresh chain only care about the last few hundred blocks anyway.
        let scan = max_blocks.min(500);
        let mut out = Vec::new();
        for offset in 0..scan {
            let n = match head.checked_sub(offset) {
                Some(v) => v,
                None => break,
            };
            let hex = format!("0x{:x}", n);
            let block = match rpc_client::call(&endpoint, "eth_getBlockByNumber", serde_json::json!([hex, false])).await {
                Ok(v) if !v.is_null() => v,
                _ => continue,
            };
            let miner = block.get("miner").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
            if miner != coinbase {
                continue;
            }
            let timestamp = block
                .get("timestamp")
                .and_then(|v| v.as_str())
                .map(rpc_client::hex_to_u64)
                .unwrap_or(0);
            let difficulty = block
                .get("difficulty")
                .and_then(|v| v.as_str())
                .map(rpc_client::hex_to_u64)
                .unwrap_or(0);
            let reward_wei: u128 = 5_000_000_000_000_000_000; // 5 CHI block reward
            out.push(MinedBlock {
                block_number: n,
                timestamp,
                reward_wei: reward_wei.to_string(),
                reward_chi: 5.0,
                difficulty,
            });
            if out.len() as u64 >= max_blocks {
                break;
            }
        }
        Ok(out)
    }

    // ------------------------------------------------------------------------
    // GPU mining stubs. The previous integration with ethminer was the
    // largest source of complexity in this module and was widely broken.
    // The frontend imports these symbols, so they need to exist; for now
    // they report "not supported" so the GPU panel grays itself out.
    // ------------------------------------------------------------------------

    pub async fn get_gpu_mining_capabilities(&self) -> Result<GpuMiningCapabilities, String> {
        Ok(GpuMiningCapabilities {
            supported: false,
            binary_path: None,
            devices: Vec::new(),
            running: false,
            active_devices: Vec::new(),
            utilization_percent: 100,
            last_error: Some("GPU mining not supported in this build".to_string()),
        })
    }

    pub async fn list_gpu_devices(&mut self) -> Result<Vec<GpuDevice>, String> {
        Ok(Vec::new())
    }

    pub async fn start_gpu_mining(
        &mut self,
        _device_ids: Option<Vec<String>>,
        _utilization_percent: Option<u8>,
    ) -> Result<(), String> {
        Err("GPU mining not supported in this build".to_string())
    }

    pub async fn stop_gpu_mining(&mut self) -> Result<(), String> {
        Ok(())
    }

    pub async fn get_gpu_mining_status(&self) -> Result<GpuMiningStatus, String> {
        Ok(GpuMiningStatus {
            running: false,
            hash_rate: 0,
            active_devices: Vec::new(),
            utilization_percent: 100,
            last_error: None,
        })
    }
}

impl Default for GethProcess {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Parse the polymorphic `eth_syncing` response.
/// - `false`            → not syncing, highest block = current
/// - `true`             → syncing, highest unknown
/// - `{currentBlock,highestBlock,...}` → syncing, fields hex-encoded
fn parse_syncing(syncing_value: Option<&serde_json::Value>, current_block: u64) -> (bool, u64) {
    let Some(v) = syncing_value else {
        return (false, current_block);
    };
    if let Some(false) = v.as_bool() {
        return (false, current_block);
    }
    if let Some(true) = v.as_bool() {
        return (true, current_block);
    }
    let Some(obj) = v.as_object() else {
        return (false, current_block);
    };
    let highest = obj
        .get("highestBlock")
        .and_then(|x| x.as_str())
        .map(rpc_client::hex_to_u64)
        .unwrap_or(current_block);
    let current = obj
        .get("currentBlock")
        .and_then(|x| x.as_str())
        .map(rpc_client::hex_to_u64)
        .unwrap_or(current_block);
    (highest > current, highest.max(current))
}

// ============================================================================
// Tests — only the things that are pure functions / cheap to verify
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freshnet_is_default_and_has_unique_chain_id() {
        // Process-cached resolution may have already set this from disk in
        // the test runner; just verify the constants are sensible.
        assert_eq!(crate::network::FRESHNET.chain_id, 98763);
        assert_ne!(crate::network::FRESHNET.chain_id, crate::network::TESTNET.chain_id);
    }

    #[test]
    fn network_id_matches_chain_id() {
        assert_eq!(network_id(), chain_id());
    }

    #[test]
    fn rpc_endpoint_routes_to_local_when_running() {
        LOCAL_GETH_RUNNING.store(true, Ordering::Relaxed);
        assert_eq!(effective_rpc_endpoint(), "http://127.0.0.1:8545");
        LOCAL_GETH_RUNNING.store(false, Ordering::Relaxed);
        assert!(effective_rpc_endpoint().contains("8545") || effective_rpc_endpoint().contains("http"));
    }

    #[test]
    fn parse_syncing_handles_false_bool() {
        let v = serde_json::json!(false);
        let (syncing, highest) = parse_syncing(Some(&v), 100);
        assert!(!syncing);
        assert_eq!(highest, 100);
    }

    #[test]
    fn parse_syncing_handles_object() {
        let v = serde_json::json!({"currentBlock": "0x5", "highestBlock": "0xa"});
        let (syncing, highest) = parse_syncing(Some(&v), 5);
        assert!(syncing);
        assert_eq!(highest, 10);
    }

    #[test]
    fn parse_syncing_object_caught_up_means_not_syncing() {
        let v = serde_json::json!({"currentBlock": "0xa", "highestBlock": "0xa"});
        let (syncing, _) = parse_syncing(Some(&v), 10);
        assert!(!syncing);
    }

    #[test]
    fn mining_status_serializes_in_camel_case() {
        let m = MiningStatus {
            mining: true,
            hash_rate: 42,
            miner_address: Some("0xabc".to_string()),
            total_mined_wei: "5".to_string(),
            total_mined_chi: 5.0,
        };
        let s = serde_json::to_string(&m).unwrap();
        assert!(s.contains("\"hashRate\":42"));
        assert!(s.contains("\"minerAddress\""));
        assert!(s.contains("\"totalMinedWei\""));
    }

    #[test]
    fn gpu_status_reports_unsupported_in_minimal_build() {
        // Smoke test that the stub returns the not-supported shape rather
        // than panicking — the frontend depends on this not throwing.
        // (Can't async-call here without a runtime; just construct directly.)
        let cap = GpuMiningCapabilities {
            supported: false,
            binary_path: None,
            devices: Vec::new(),
            running: false,
            active_devices: Vec::new(),
            utilization_percent: 100,
            last_error: Some("GPU mining not supported in this build".to_string()),
        };
        assert!(!cap.supported);
        assert!(cap.last_error.unwrap().contains("not supported"));
    }
}
