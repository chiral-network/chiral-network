//! Geth Management Module for Chiral Network
//!
//! This module handles:
//! - Downloading Core-Geth binary
//! - Starting/stopping Geth process
//! - Genesis initialization
//! - RPC communication
//! - Bootstrap node health checking (via geth_bootstrap module)

use crate::geth_bootstrap;
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
// Configuration
// ============================================================================

/// Chiral Network chain ID
pub const CHAIN_ID: u64 = 98765;

/// Network ID (same as chain ID for our network)
pub const NETWORK_ID: u64 = 98765;

/// Tracks whether a local Geth process is running (set by GethProcess start/stop)
static LOCAL_GETH_RUNNING: AtomicBool = AtomicBool::new(false);

/// Returns the effective RPC endpoint without requiring a lock on GethProcess.
/// Uses the atomic LOCAL_GETH_RUNNING flag set by start/stop.
pub fn effective_rpc_endpoint() -> String {
    if LOCAL_GETH_RUNNING.load(Ordering::Relaxed) {
        "http://127.0.0.1:8545".to_string()
    } else {
        rpc_endpoint()
    }
}

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
        self.bin_dir().join(if cfg!(target_os = "windows") {
            "geth.exe"
        } else {
            "geth"
        })
    }

    pub fn bin_dir(&self) -> PathBuf {
        self.base_dir.join("bin")
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
            return Err(format!(
                "Download failed with status: {}",
                response.status()
            ));
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
        let mut archive = zip::ZipArchive::new(reader)
            .map_err(|e| format!("Failed to read zip archive: {}", e))?;

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
    gpu_miner_child: Option<Child>,
    gpu_binary_path: Option<PathBuf>,
    gpu_auto_install_attempted: bool,
    gpu_backend: Option<String>,
    gpu_active_devices: Vec<String>,
    gpu_utilization_percent: u8,
    gpu_hash_rate: u64,
    gpu_last_error: Option<String>,
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
            gpu_miner_child: None,
            gpu_binary_path: Self::discover_gpu_miner_binary(),
            gpu_auto_install_attempted: false,
            gpu_backend: None,
            gpu_active_devices: Vec::new(),
            gpu_utilization_percent: 100,
            gpu_hash_rate: 0,
            gpu_last_error: None,
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
        if let Ok(output) = Command::new("fuser")
            .args(["8545/tcp"])
            .stderr(Stdio::piped())
            .output()
        {
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
                Command::new("kill")
                    .args(["-0", &pid_s])
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false)
            };

            if !is_alive() {
                continue;
            }

            println!("⚠️  Killing orphaned Geth (PID {}), sending SIGTERM", pid);
            let _ = Command::new("kill").arg(&pid_s).output();

            // Wait up to 1.5s for graceful exit (3 × 500ms instead of 10 × 500ms)
            let mut exited = false;
            for i in 0..3 {
                std::thread::sleep(std::time::Duration::from_millis(500));
                if !is_alive() {
                    println!(
                        "✅ Orphaned Geth (PID {}) exited after {}ms",
                        pid,
                        (i + 1) * 500
                    );
                    exited = true;
                    break;
                }
            }

            if !exited {
                println!("⚠️  SIGTERM failed, sending SIGKILL to PID {}", pid);
                let _ = Command::new("kill").args(["-9", &pid_s]).output();
                // Brief wait for SIGKILL to take effect
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }

        // Clean up geth.ipc — just remove it directly instead of polling
        if ipc_path.exists() {
            let _ = fs::remove_file(&ipc_path);
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

    fn configured_sync_mode() -> String {
        let mode = std::env::var("CHIRAL_GETH_SYNCMODE")
            .unwrap_or_else(|_| "full".to_string())
            .trim()
            .to_ascii_lowercase();

        match mode.as_str() {
            "snap" | "full" => mode,
            other => {
                println!(
                    "⚠️ Unsupported CHIRAL_GETH_SYNCMODE '{}', falling back to full",
                    other
                );
                "full".to_string()
            }
        }
    }

    fn configured_cache_mb() -> u32 {
        let parsed = std::env::var("CHIRAL_GETH_CACHE_MB")
            .ok()
            .and_then(|raw| raw.trim().parse::<u32>().ok())
            .unwrap_or(2048);
        parsed.clamp(512, 4096)
    }

    pub fn is_running(&self) -> bool {
        self.child.is_some()
    }

    pub fn geth_path(&self) -> PathBuf {
        self.downloader.geth_path()
    }

    fn gpu_log_path(&self) -> PathBuf {
        self.data_dir.join("gpu-miner.log")
    }

    fn gpu_binary_name() -> &'static str {
        if cfg!(target_os = "windows") {
            "ethminer.exe"
        } else {
            "ethminer"
        }
    }

    fn default_gpu_binary_path(&self) -> PathBuf {
        self.downloader.bin_dir().join(Self::gpu_binary_name())
    }

    fn gpu_download_url() -> Result<&'static str, String> {
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("windows", "x86_64") => Ok("https://github.com/ethereum-mining/ethminer/releases/download/v0.18.0/ethminer-0.18.0-cuda10.0-windows-amd64.zip"),
            ("linux", "x86_64") => Ok("https://github.com/ethereum-mining/ethminer/releases/download/v0.18.0/ethminer-0.18.0-cuda-9-linux-x86_64.tar.gz"),
            ("macos", "x86_64") | ("macos", "aarch64") => Ok("https://github.com/ethereum-mining/ethminer/releases/download/v0.18.0/ethminer-0.18.0-cuda-9-darwin-x86_64.tar.gz"),
            _ => Err(format!(
                "GPU miner auto-install is not supported on platform {} {}",
                std::env::consts::OS,
                std::env::consts::ARCH
            )),
        }
    }

    fn extract_gpu_miner_zip(data: &[u8], output_dir: &Path) -> Result<PathBuf, String> {
        let reader = Cursor::new(data);
        let mut archive = zip::ZipArchive::new(reader)
            .map_err(|e| format!("Failed to read zip archive: {}", e))?;
        let binary_name = Self::gpu_binary_name();

        for i in 0..archive.len() {
            let mut file = archive
                .by_index(i)
                .map_err(|e| format!("Failed to read zip entry: {}", e))?;
            let entry_name = Path::new(file.name())
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            if entry_name != binary_name {
                continue;
            }

            let out_path = output_dir.join(binary_name);
            let mut outfile = fs::File::create(&out_path)
                .map_err(|e| format!("Failed to create GPU miner file: {}", e))?;
            std::io::copy(&mut file, &mut outfile)
                .map_err(|e| format!("Failed to write GPU miner file: {}", e))?;
            return Ok(out_path);
        }

        Err("Could not find GPU miner binary in zip archive".to_string())
    }

    fn extract_gpu_miner_targz(data: &[u8], output_dir: &Path) -> Result<PathBuf, String> {
        let decoder = GzDecoder::new(Cursor::new(data));
        let mut archive = tar::Archive::new(decoder);
        let binary_name = Self::gpu_binary_name();

        let entries = archive
            .entries()
            .map_err(|e| format!("Failed to read tar archive entries: {}", e))?;
        for entry in entries {
            let mut entry = entry.map_err(|e| format!("Failed to read tar entry: {}", e))?;
            let entry_path = entry
                .path()
                .map_err(|e| format!("Failed to read tar entry path: {}", e))?;
            let file_name = entry_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            if file_name != binary_name {
                continue;
            }

            let out_path = output_dir.join(binary_name);
            let mut outfile = fs::File::create(&out_path)
                .map_err(|e| format!("Failed to create GPU miner file: {}", e))?;
            std::io::copy(&mut entry, &mut outfile)
                .map_err(|e| format!("Failed to write GPU miner file: {}", e))?;
            return Ok(out_path);
        }

        Err("Could not find GPU miner binary in tar.gz archive".to_string())
    }

    async fn ensure_gpu_miner_available(&mut self) -> Result<PathBuf, String> {
        if let Some(found) = Self::discover_gpu_miner_binary() {
            self.gpu_binary_path = Some(found.clone());
            self.gpu_last_error = None;
            return Ok(found);
        }

        if self.gpu_auto_install_attempted {
            return Err(self.gpu_last_error.clone().unwrap_or_else(|| {
                "GPU miner auto-install already attempted but binary is still unavailable"
                    .to_string()
            }));
        }
        self.gpu_auto_install_attempted = true;

        let url = Self::gpu_download_url()?;
        let bin_dir = self.downloader.bin_dir();
        fs::create_dir_all(&bin_dir)
            .map_err(|e| format!("Failed to create GPU miner install directory: {}", e))?;

        println!("⛏️  GPU miner not found; auto-downloading from {}", url);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))?;
        let response = client
            .get(url)
            .send()
            .await
            .map_err(|e| format!("Failed to download GPU miner: {}", e))?;
        if !response.status().is_success() {
            return Err(format!(
                "Failed to download GPU miner archive: HTTP {}",
                response.status()
            ));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("Failed reading GPU miner download: {}", e))?;
        let _installed_path = if url.ends_with(".zip") {
            Self::extract_gpu_miner_zip(&bytes, &bin_dir)?
        } else if url.ends_with(".tar.gz") {
            Self::extract_gpu_miner_targz(&bytes, &bin_dir)?
        } else {
            return Err("Unsupported GPU miner archive format".to_string());
        };

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&_installed_path)
                .map_err(|e| format!("Failed to read GPU miner metadata: {}", e))?
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&_installed_path, perms)
                .map_err(|e| format!("Failed to set GPU miner permissions: {}", e))?;
        }

        let found = Self::discover_gpu_miner_binary().or_else(|| {
            if self.default_gpu_binary_path().exists() {
                Some(self.default_gpu_binary_path())
            } else {
                None
            }
        });
        let Some(path) = found else {
            return Err(
                "GPU miner download completed but executable could not be located".to_string(),
            );
        };
        self.gpu_binary_path = Some(path.clone());
        self.gpu_last_error = None;
        println!("⛏️  GPU miner installed at {}", path.display());
        Ok(path)
    }

    fn discover_gpu_miner_binary() -> Option<PathBuf> {
        if let Ok(path) = std::env::var("CHIRAL_GPU_MINER_PATH") {
            let p = PathBuf::from(path);
            if p.exists() {
                return Some(p);
            }
        }

        let current_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));

        let candidates = if cfg!(target_os = "windows") {
            vec![
                current_dir.join("bin").join("ethminer.exe"),
                current_dir.join("ethminer.exe"),
            ]
        } else {
            vec![
                current_dir.join("bin").join("ethminer"),
                current_dir.join("ethminer"),
            ]
        };

        for candidate in candidates {
            if candidate.exists() {
                return Some(candidate);
            }
        }

        let path_names: &[&str] = if cfg!(target_os = "windows") {
            &["ethminer.exe", "ethminer"]
        } else {
            &["ethminer"]
        };
        for name in path_names {
            if Command::new(name)
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok()
            {
                return Some(PathBuf::from(name));
            }
        }

        None
    }

    fn parse_gpu_devices_from_text(output: &str) -> Vec<GpuDevice> {
        fn parse_bracket_format(trimmed: &str) -> Option<GpuDevice> {
            if !trimmed.starts_with('[') {
                return None;
            }
            let end_idx = trimmed.find(']')?;
            let id = trimmed[1..end_idx].trim();
            if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let mut name = trimmed[end_idx + 1..].trim();
            if let Some(stripped) = name.strip_prefix(':') {
                name = stripped.trim();
            }
            if name.is_empty() {
                return None;
            }
            Some(GpuDevice {
                id: id.to_string(),
                name: name.to_string(),
            })
        }

        fn token_is_numeric(token: &str) -> bool {
            !token.is_empty()
                && token
                    .chars()
                    .all(|c| c.is_ascii_digit() || c == '.' || c == ',')
        }

        fn token_is_size_unit(token: &str) -> bool {
            matches!(
                token.to_ascii_lowercase().as_str(),
                "mb" | "gb" | "kb" | "mib" | "gib" | "kib"
            )
        }

        fn parse_tabular_format(trimmed: &str) -> Option<GpuDevice> {
            let columns: Vec<&str> = trimmed.split_whitespace().collect();
            if columns.len() < 2 {
                return None;
            }
            if !columns[0].chars().all(|c| c.is_ascii_digit()) {
                return None;
            }

            // Common ethminer table formats:
            //   "<id> <pci> <type> <name...> <metrics...>"
            //   "<id> <pci> <name...> <metrics...>"
            //   "<id> <name...>"
            let looks_like_pci = |token: &str| token.contains(':') || token.contains('.');
            let start_idx = if columns.len() >= 4
                && looks_like_pci(columns[1])
                && columns[2].chars().all(|c| c.is_ascii_alphabetic())
            {
                3
            } else if columns.len() >= 3 && looks_like_pci(columns[1]) {
                2
            } else {
                1
            };
            if columns.len() <= start_idx {
                return None;
            }

            // Trim known metric suffixes without stripping numeric model names
            // like "RTX 4090".
            let mut end_idx = columns.len();
            if end_idx > start_idx + 1
                && token_is_size_unit(columns[end_idx - 1])
                && token_is_numeric(columns[end_idx - 2])
            {
                end_idx -= 2; // "... <total_mem> MB"
            }
            if end_idx > start_idx {
                let tail = columns[end_idx - 1].to_ascii_lowercase();
                if token_is_numeric(&tail) || matches!(tail.as_str(), "n/a" | "na" | "-") {
                    end_idx -= 1; // "... <cuda_sm>"
                }
            }
            if end_idx <= start_idx {
                end_idx = columns.len();
            }
            let name = columns[start_idx..end_idx].join(" ");
            if name.is_empty() {
                return None;
            }

            Some(GpuDevice {
                id: columns[0].to_string(),
                name,
            })
        }

        let mut out: Vec<GpuDevice> = Vec::new();
        for line in output.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(device) =
                parse_bracket_format(trimmed).or_else(|| parse_tabular_format(trimmed))
            {
                if !out
                    .iter()
                    .any(|existing| existing.id == device.id && existing.name == device.name)
                {
                    out.push(device);
                }
            }
        }
        out
    }

    fn scan_gpu_devices(
        binary_path: &Path,
        backend_flag: &str,
    ) -> Result<(Vec<GpuDevice>, String), String> {
        let output = Command::new(binary_path)
            .arg(backend_flag)
            .arg("--list-devices")
            .output()
            .map_err(|e| format!("Failed to query GPU devices ({}): {}", backend_flag, e))?;

        let mut text = String::new();
        text.push_str(&String::from_utf8_lossy(&output.stdout));
        if !output.stderr.is_empty() {
            text.push('\n');
            text.push_str(&String::from_utf8_lossy(&output.stderr));
        }
        let devices = Self::parse_gpu_devices_from_text(&text);
        Ok((devices, text))
    }

    fn clamp_gpu_utilization_percent(utilization_percent: Option<u8>) -> u8 {
        utilization_percent.unwrap_or(100).clamp(10, 100)
    }

    fn gpu_tuning_args(backend: &str, utilization_percent: u8) -> Vec<String> {
        if utilization_percent >= 100 {
            return Vec::new();
        }

        if backend == "cuda" {
            // Lower grid size/stream count to cap GPU occupancy.
            let mut grid = 32_768u32.saturating_mul(utilization_percent as u32) / 100;
            if grid < 1_024 {
                grid = 1_024;
            }
            grid = (grid / 256).max(1) * 256;
            let streams = if utilization_percent >= 70 { 2 } else { 1 };
            vec![
                "--cuda-grid-size".to_string(),
                grid.to_string(),
                "--cuda-streams".to_string(),
                streams.to_string(),
            ]
        } else {
            // Lower OpenCL global/local work size to reduce device pressure.
            let mut global_work = 32_768u32.saturating_mul(utilization_percent as u32) / 100;
            if global_work < 4_096 {
                global_work = 4_096;
            }
            global_work = (global_work / 64).max(1) * 64;
            let local_work = if utilization_percent >= 70 { 128 } else { 64 };
            vec![
                "--opencl-global-work".to_string(),
                global_work.to_string(),
                "--opencl-local-work".to_string(),
                local_work.to_string(),
            ]
        }
    }

    fn read_log_tail(path: &Path, max_lines: usize) -> String {
        let Ok(contents) = fs::read_to_string(path) else {
            return String::new();
        };
        let mut tail = contents.lines().rev().take(max_lines).collect::<Vec<_>>();
        tail.reverse();
        tail.join("\n")
    }

    fn gpu_log_indicates_activity(contents: &str) -> bool {
        let activity_markers = [
            "new job",
            "job:",
            "got work package",
            "work package",
            "mining on",
            "speed",
            "searching for solutions",
            "solution",
            "accepted",
            "epoch",
            "dag",
        ];
        contents.lines().any(|line| {
            if Self::parse_hash_rate_from_line(line).is_some() {
                return true;
            }
            let lower = line.to_ascii_lowercase();
            activity_markers.iter().any(|m| lower.contains(m))
        })
    }

    fn gpu_log_fatal_error(contents: &str) -> Option<String> {
        let fatal_markers = [
            "unrecognized option",
            "invalid argument",
            "unknown option",
            "no usable mining devices found",
            "no opencl platforms found",
            "no cuda-capable device",
            "cuda error",
            "opencl error",
            "clcreatecontext",
            "failed to initialize",
            "json-rpc problem",
            "connection refused",
        ];
        for line in contents.lines().rev().take(80) {
            let lower = line.to_ascii_lowercase();
            if fatal_markers.iter().any(|m| lower.contains(m)) {
                return Some(line.trim().to_string());
            }
        }
        None
    }

    fn parse_hash_rate_from_line(line: &str) -> Option<u64> {
        let lower = line.to_ascii_lowercase();
        let units = [
            ("gh/s", 1_000_000_000f64),
            ("mh/s", 1_000_000f64),
            ("kh/s", 1_000f64),
            ("h/s", 1f64),
        ];
        for (unit, mult) in units {
            if let Some(idx) = lower.rfind(unit) {
                let before = &lower[..idx];
                if let Some(token) = before.split_whitespace().last() {
                    let cleaned =
                        token.trim_matches(|c: char| !(c.is_ascii_digit() || c == '.' || c == ','));
                    let normalized = cleaned.replace(',', ".");
                    if let Ok(value) = normalized.parse::<f64>() {
                        let h = (value * mult) as u64;
                        if h > 0 {
                            return Some(h);
                        }
                    }
                }
            }
        }
        None
    }

    fn refresh_gpu_hash_rate_from_log(&mut self) {
        let path = self.gpu_log_path();
        let Ok(contents) = fs::read_to_string(path) else {
            return;
        };
        for line in contents.lines().rev() {
            if let Some(rate) = Self::parse_hash_rate_from_line(line) {
                self.gpu_hash_rate = rate;
                break;
            }
        }
    }

    fn refresh_gpu_runtime(&mut self) {
        if let Some(ref mut child) = self.gpu_miner_child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.gpu_miner_child = None;
                    if !status.success() {
                        self.gpu_last_error = Some(format!(
                            "GPU miner exited unexpectedly with status {}",
                            status
                        ));
                    }
                }
                Ok(None) => {}
                Err(err) => {
                    self.gpu_miner_child = None;
                    self.gpu_last_error =
                        Some(format!("Failed to inspect GPU miner process: {}", err));
                }
            }
        }

        if self.gpu_miner_child.is_some() {
            self.refresh_gpu_hash_rate_from_log();
        } else {
            self.gpu_hash_rate = 0;
        }
    }

    fn stop_gpu_miner_sync(&mut self) -> Result<(), String> {
        if let Some(mut child) = self.gpu_miner_child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.gpu_backend = None;
        self.gpu_hash_rate = 0;
        self.gpu_active_devices.clear();
        self.gpu_utilization_percent = 100;
        Ok(())
    }

    fn stop_gpu_miner_fast_sync(&mut self) {
        if let Some(mut child) = self.gpu_miner_child.take() {
            let _ = child.kill();
        }
        self.gpu_backend = None;
        self.gpu_hash_rate = 0;
        self.gpu_active_devices.clear();
        self.gpu_utilization_percent = 100;
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
            "difficulty": "0x400000",
            "gasLimit": "0x47b760",
            "alloc": {},
            "coinbase": "0x0000000000000000000000000000000000000000",
            "extraData": "0x4b656570206f6e206b656570696e67206f6e21",
            "nonce": "0x0000000000000042",
            "mixhash": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "parentHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "timestamp": "0x68b3b2ca"
        })
        .to_string()
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
        if let Ok(output) = Command::new("fuser")
            .args(["8545/tcp"])
            .stderr(Stdio::piped())
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stdout.trim().is_empty() || !stderr.trim().is_empty() {
                println!(
                    "⚠️  Port 8545 still in use! stdout='{}' stderr='{}'",
                    stdout.trim(),
                    stderr.trim()
                );
            } else {
                println!("✅ Port 8545 is free");
            }
        }

        // Debug: check for LOCK files after cleanup
        if let Ok(entries) = fs::read_dir(self.data_dir.join("geth")) {
            for entry in entries.flatten() {
                if entry.file_name() == "LOCK" {
                    println!(
                        "⚠️  LOCK file still exists after cleanup: {}",
                        entry.path().display()
                    );
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

        // Check if blockchain needs initialization or re-initialization.
        // Use a version marker to detect genesis config changes.
        let genesis_version = "5"; // Bump this when genesis config changes
        let version_file = self.data_dir.join(".genesis_version");
        let chaindata_path = self.data_dir.join("geth").join("chaindata");

        let needs_init = if !chaindata_path.exists() {
            println!("No chaindata found — need genesis init");
            true
        } else {
            match fs::read_to_string(&version_file) {
                Ok(v) if v.trim() == genesis_version => false, // Version matches, chain data is good
                Ok(v) => {
                    // Version mismatch — genesis config changed, must re-init
                    println!("Genesis version mismatch (have '{}', need '{}') — re-initializing", v.trim(), genesis_version);
                    true
                }
                Err(_) => {
                    // No version file but chaindata exists — write the version file
                    // without wiping chain data (assume it's compatible).
                    // This prevents accidental chain wipes when the version file
                    // is missing due to migration or filesystem issues.
                    println!("No .genesis_version file but chaindata exists — assuming compatible, writing version marker");
                    let _ = fs::write(&version_file, genesis_version);
                    false
                }
            }
        };

        if needs_init {
            println!("Initializing blockchain (genesis v{})...", genesis_version);
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
        println!(
            "✅ Using bootstrap nodes: {}",
            if bootstrap_nodes.len() > 100 {
                format!("{}...", &bootstrap_nodes[..100])
            } else {
                bootstrap_nodes.clone()
            }
        );

        let sync_mode = Self::configured_sync_mode();
        let cache_mb = Self::configured_cache_mb();

        let mut cmd = Command::new(&geth_path);
        cmd.arg("--datadir")
            .arg(&self.data_dir)
            .arg("--networkid")
            .arg(NETWORK_ID.to_string())
            .arg("--http")
            .arg("--http.addr")
            .arg("127.0.0.1") // Only allow local RPC connections (security)
            .arg("--http.port")
            .arg("8545")
            .arg("--http.api")
            .arg("eth,net,web3,personal,debug,miner,admin,txpool")
            .arg("--http.corsdomain")
            .arg("*")
            .arg("--syncmode")
            .arg(&sync_mode) // Prefer snap sync for faster catch-up; overridable via env
            .arg("--gcmode")
            .arg("archive") // Keep all state to prevent block height regression on restart
            .arg("--cache")
            .arg(cache_mb.to_string())
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
            cmd.arg("--miner.etherbase")
                .arg(addr)
                .arg("--mine")
                .arg("--miner.threads")
                .arg("1");
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
                    println!(
                        "❌ Geth crashed on startup (exit: {}):\n{}",
                        status, crash_log
                    );
                    return Err(format!(
                        "Geth crashed on startup (exit: {}). Check logs:\n{}",
                        status, crash_log
                    ));
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
        let _ = self.stop_gpu_miner_sync();

        if let Some(mut child) = self.child.take() {
            LOCAL_GETH_RUNNING.store(false, Ordering::Relaxed);

            let pid = child.id();

            // Send SIGTERM first for graceful shutdown (releases LOCK files properly)
            let _ = Command::new("kill").arg(pid.to_string()).output();

            // Wait up to 2 seconds for graceful exit
            let mut exited = false;
            for _ in 0..4 {
                std::thread::sleep(std::time::Duration::from_millis(500));
                match child.try_wait() {
                    Ok(Some(_)) => {
                        exited = true;
                        break;
                    }
                    Ok(None) => {} // still running
                    Err(_) => {
                        exited = true;
                        break;
                    }
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

    /// Fast stop used during app shutdown.
    /// Prioritizes quick process termination over graceful wait loops.
    pub fn stop_fast(&mut self) -> Result<(), String> {
        self.stop_gpu_miner_fast_sync();

        if let Some(mut child) = self.child.take() {
            LOCAL_GETH_RUNNING.store(false, Ordering::Relaxed);

            let pid = child.id();
            let _ = Command::new("kill").arg(pid.to_string()).output();

            // Only wait briefly; if still running, force kill immediately.
            let mut exited = false;
            for _ in 0..2 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                match child.try_wait() {
                    Ok(Some(_)) => {
                        exited = true;
                        break;
                    }
                    Ok(None) => {}
                    Err(_) => {
                        exited = true;
                        break;
                    }
                }
            }

            if !exited {
                let _ = child.kill();
            }

            let pid_path = self.data_dir.join("geth.pid");
            let _ = fs::remove_file(&pid_path);
            println!("✅ Geth stopped (fast)");
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

        let endpoint = self.effective_rpc_endpoint();

        // Batch: eth_syncing + eth_blockNumber + net_peerCount + eth_chainId in one request
        let mut batch = crate::rpc_client::batch();
        let sync_idx = batch.add("eth_syncing", serde_json::json!([]));
        let block_idx = batch.add("eth_blockNumber", serde_json::json!([]));
        let peers_idx = batch.add("net_peerCount", serde_json::json!([]));
        let chain_idx = batch.add("eth_chainId", serde_json::json!([]));

        let results = batch.execute(&endpoint).await?;

        // Parse syncing
        let (syncing, sync_current, sync_highest) = match results[sync_idx].as_ref().ok() {
            Some(result) => {
                if result.is_boolean() && !result.as_bool().unwrap_or(false) {
                    (false, 0u64, 0u64)
                } else if result.is_null() {
                    (false, 0, 0)
                } else if let Some(obj) = result.as_object() {
                    let current = crate::rpc_client::hex_to_u64(
                        obj.get("currentBlock")
                            .and_then(|v| v.as_str())
                            .unwrap_or("0x0"),
                    );
                    let highest = crate::rpc_client::hex_to_u64(
                        obj.get("highestBlock")
                            .and_then(|v| v.as_str())
                            .unwrap_or("0x0"),
                    );
                    let actually_syncing = highest > 0 && current < highest;
                    (actually_syncing, current, highest)
                } else {
                    (false, 0, 0)
                }
            }
            None => (false, 0, 0),
        };

        let block_number = results[block_idx]
            .as_ref()
            .ok()
            .and_then(|v| v.as_str())
            .map(crate::rpc_client::hex_to_u64)
            .unwrap_or(0);

        let peer_count = results[peers_idx]
            .as_ref()
            .ok()
            .and_then(|v| v.as_str())
            .map(|h| crate::rpc_client::hex_to_u64(h) as u32)
            .unwrap_or(0);

        let chain_id = results[chain_idx]
            .as_ref()
            .ok()
            .and_then(|v| v.as_str())
            .map(crate::rpc_client::hex_to_u64)
            .unwrap_or(0);

        Ok(GethStatus {
            installed: self.is_installed(),
            running: self.child.is_some(),
            local_running: self.child.is_some(),
            syncing,
            current_block: if syncing { sync_current } else { block_number },
            highest_block: if syncing { sync_highest } else { block_number },
            peer_count,
            chain_id,
        })
    }

    /// Start mining (requires local Geth process)
    pub async fn start_mining(&mut self, threads: u32) -> Result<(), String> {
        // Check if local Geth process is still alive
        if let Some(ref mut child) = self.child {
            match child.try_wait() {
                Ok(Some(_)) => {
                    self.child = None;
                    LOCAL_GETH_RUNNING.store(false, Ordering::Relaxed);
                    return Err("Cannot mine: local Geth node has stopped. Restart it from the Network page.".to_string());
                }
                Err(_) => {
                    self.child = None;
                    LOCAL_GETH_RUNNING.store(false, Ordering::Relaxed);
                    return Err("Cannot mine: local Geth node is not responding. Restart it from the Network page.".to_string());
                }
                Ok(None) => {} // Still running
            }
        } else {
            return Err("Cannot mine: local Geth node is not running. Start the node from the Network page first.".to_string());
        }

        // CPU and GPU mining modes are mutually exclusive.
        self.stop_gpu_miner_sync()?;

        let endpoint = self.effective_rpc_endpoint();

        // Ensure etherbase is set — miner_start won't mine without a coinbase
        let coinbase = crate::rpc_client::call(&endpoint, "eth_coinbase", serde_json::json!([]))
            .await;
        match coinbase {
            Ok(ref val) => {
                let addr = val.as_str().unwrap_or("");
                if addr.is_empty() || addr == "0x0000000000000000000000000000000000000000" {
                    return Err(
                        "Cannot mine: no miner address (coinbase) is set. Please set your wallet address first."
                            .to_string(),
                    );
                }
            }
            Err(_) => {
                return Err("Cannot mine: failed to verify miner address".to_string());
            }
        }

        // Stop then start to ensure thread count takes effect
        let _ = crate::rpc_client::call(&endpoint, "miner_stop", serde_json::json!([])).await;
        crate::rpc_client::call(&endpoint, "miner_start", serde_json::json!([threads]))
            .await
            .map(|_| ())
    }

    /// Stop mining
    pub async fn stop_mining(&mut self) -> Result<(), String> {
        let endpoint = self.effective_rpc_endpoint();
        crate::rpc_client::call(&endpoint, "miner_stop", serde_json::json!([]))
            .await
            .map(|_| ())
    }

    /// Returns GPU mining capability details (binary + detected devices + runtime state).
    pub async fn get_gpu_mining_capabilities(&mut self) -> Result<GpuMiningCapabilities, String> {
        self.refresh_gpu_runtime();

        if self.gpu_binary_path.is_none() {
            match self.ensure_gpu_miner_available().await {
                Ok(path) => {
                    self.gpu_binary_path = Some(path);
                }
                Err(err) => {
                    self.gpu_last_error = Some(err);
                }
            }
        }
        let binary = self.gpu_binary_path.clone();
        let Some(binary_path) = binary else {
            return Ok(GpuMiningCapabilities {
                supported: false,
                binary_path: None,
                devices: Vec::new(),
                running: false,
                active_devices: self.gpu_active_devices.clone(),
                utilization_percent: self.gpu_utilization_percent,
                last_error: self.gpu_last_error.clone(),
            });
        };

        let mut devices: Vec<GpuDevice> = Vec::new();
        let mut backend: Option<String> = None;
        let mut scan_messages: Vec<String> = Vec::new();

        // On macOS there is no CUDA support — try OpenCL only.
        // On other platforms try CUDA first, then fall back to OpenCL.
        let backends: Vec<(&str, &str)> = if cfg!(target_os = "macos") {
            vec![("opencl", "-G")]
        } else {
            vec![("cuda", "-U"), ("opencl", "-G")]
        };
        for (backend_name, flag) in backends {
            match Self::scan_gpu_devices(&binary_path, flag) {
                Ok((found, raw)) => {
                    if !found.is_empty() {
                        devices = found;
                        backend = Some(backend_name.to_string());
                        break;
                    }
                    let raw_excerpt = raw.lines().take(6).collect::<Vec<_>>().join(" | ");
                    scan_messages.push(format!("{}: no devices ({})", backend_name, raw_excerpt));
                }
                Err(err) => {
                    scan_messages.push(format!("{} scan failed: {}", backend_name, err));
                }
            }
        }

        self.gpu_backend = backend.clone();
        if backend.is_none() {
            self.gpu_last_error = Some(format!(
                "GPU miner could not find devices. {}",
                scan_messages.join(" ; ")
            ));
        } else {
            self.gpu_last_error = None;
        }

        Ok(GpuMiningCapabilities {
            supported: backend.is_some(),
            binary_path: Some(binary_path.to_string_lossy().to_string()),
            devices,
            running: self.gpu_miner_child.is_some(),
            active_devices: self.gpu_active_devices.clone(),
            utilization_percent: self.gpu_utilization_percent,
            last_error: self.gpu_last_error.clone(),
        })
    }

    pub async fn list_gpu_devices(&mut self) -> Result<Vec<GpuDevice>, String> {
        Ok(self.get_gpu_mining_capabilities().await?.devices)
    }

    pub async fn start_gpu_mining(
        &mut self,
        device_ids: Option<Vec<String>>,
        utilization_percent: Option<u8>,
    ) -> Result<(), String> {
        if self.child.is_none() {
            return Err(
                "Cannot start GPU miner: local Geth node is not running. Start the node from the Network page first."
                    .to_string(),
            );
        }

        self.refresh_gpu_runtime();
        if self.gpu_miner_child.is_some() {
            return Err("GPU miner is already running".to_string());
        }

        // Ensure geth serves fresh work packages for external miners.
        let endpoint = self.effective_rpc_endpoint();
        crate::rpc_client::call(&endpoint, "miner_start", serde_json::json!([1]))
            .await
            .map_err(|e| {
                format!(
                    "Failed to enable local mining work packages for GPU miner: {}",
                    e
                )
            })?;

        if self.gpu_binary_path.is_none() {
            let binary = self
                .ensure_gpu_miner_available()
                .await
                .map_err(|e| format!("GPU miner unavailable: {}", e))?;
            self.gpu_binary_path = Some(binary);
        }
        let binary = self.gpu_binary_path.clone().ok_or_else(|| {
            "No GPU miner binary found after auto-install. Set CHIRAL_GPU_MINER_PATH and retry."
                .to_string()
        })?;

        let selected = device_ids.unwrap_or_default();
        let utilization_percent = Self::clamp_gpu_utilization_percent(utilization_percent);
        let log_path = self.gpu_log_path();
        if let Some(parent) = log_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create GPU miner log directory: {}", e))?;
        }
        let preferred_backend = self.gpu_backend.clone();
        let mut backend_attempts: Vec<String> = match preferred_backend.as_deref() {
            Some("cuda") => vec!["cuda".to_string(), "opencl".to_string()],
            Some("opencl") => vec!["opencl".to_string(), "cuda".to_string()],
            _ => {
                // macOS has no CUDA — default to OpenCL only.
                if cfg!(target_os = "macos") {
                    vec!["opencl".to_string()]
                } else {
                    vec!["cuda".to_string(), "opencl".to_string()]
                }
            }
        };
        backend_attempts.dedup();

        let mut failures: Vec<String> = Vec::new();
        for backend in backend_attempts {
            let selection_attempts: Vec<Option<Vec<String>>> = if selected.is_empty() {
                vec![None]
            } else {
                vec![Some(selected.clone()), None]
            };

            for (attempt_index, selected_override) in selection_attempts.into_iter().enumerate() {
                let selected_for_attempt = selected_override.unwrap_or_default();
                let tuning_args = Self::gpu_tuning_args(&backend, utilization_percent);
                let tuning_attempts: Vec<(Vec<String>, &str)> = if tuning_args.is_empty() {
                    vec![(Vec::new(), "default tuning")]
                } else {
                    vec![
                        (tuning_args, "requested utilization"),
                        (Vec::new(), "fallback without tuning"),
                    ]
                };

                for (tuning_index, (extra_args, tuning_note)) in
                    tuning_attempts.into_iter().enumerate()
                {
                    let log_file = OpenOptions::new()
                        .create(true)
                        .write(true)
                        .truncate(failures.is_empty() && attempt_index == 0 && tuning_index == 0)
                        .append(!(failures.is_empty() && attempt_index == 0 && tuning_index == 0))
                        .open(&log_path)
                        .map_err(|e| format!("Failed to open GPU miner log file: {}", e))?;
                    let log_clone = log_file
                        .try_clone()
                        .map_err(|e| format!("Failed to clone GPU miner log file handle: {}", e))?;

                    let mut cmd = Command::new(&binary);
                    if backend == "cuda" {
                        cmd.arg("-U");
                    } else {
                        cmd.arg("-G");
                    }
                    cmd.arg("-P")
                        .arg("http://127.0.0.1:8545")
                        .arg("--farm-recheck")
                        .arg("200")
                        .args(&extra_args);

                    if !selected_for_attempt.is_empty() {
                        let joined = selected_for_attempt.join(",");
                        if backend == "cuda" {
                            cmd.arg("--cuda-devices").arg(&joined);
                        } else {
                            cmd.arg("--opencl-devices").arg(&joined);
                        }
                    }

                    cmd.stdout(Stdio::from(log_clone))
                        .stderr(Stdio::from(log_file));

                    match cmd.spawn() {
                        Ok(child) => {
                            self.gpu_miner_child = Some(child);
                            self.gpu_active_devices = selected_for_attempt.clone();
                            self.gpu_utilization_percent = utilization_percent;
                            self.gpu_hash_rate = 0;
                            self.gpu_last_error = None;
                        }
                        Err(err) => {
                            failures.push(format!(
                                "{} backend failed to spawn ({}): {}",
                                backend, tuning_note, err
                            ));
                            continue;
                        }
                    }

                    let mut startup_verified = false;
                    let mut startup_error: Option<String> = None;
                    for _ in 0..25 {
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        self.refresh_gpu_runtime();
                        let log_tail = Self::read_log_tail(&log_path, 80);

                        if let Some(fatal_line) = Self::gpu_log_fatal_error(&log_tail) {
                            startup_error = Some(fatal_line);
                            break;
                        }
                        if self.gpu_miner_child.is_none() {
                            break;
                        }
                        if self.gpu_hash_rate > 0 || Self::gpu_log_indicates_activity(&log_tail) {
                            startup_verified = true;
                            break;
                        }
                    }

                    if startup_verified && self.gpu_miner_child.is_some() {
                        self.gpu_backend = Some(backend.clone());
                        self.gpu_last_error = None;
                        return Ok(());
                    }

                    // Some systems are slow to emit hashrate/work logs (DAG build, buffered logs).
                    // If the process is still alive and we didn't observe a fatal error, treat startup
                    // as successful and keep a warning instead of failing fast.
                    if self.gpu_miner_child.is_some() && startup_error.is_none() {
                        self.gpu_backend = Some(backend.clone());
                        self.gpu_last_error = Some(
                            "GPU miner started, but hashrate is not visible yet. This can happen during DAG initialization."
                                .to_string(),
                        );
                        return Ok(());
                    }

                    let log_tail = Self::read_log_tail(&log_path, 40);
                    let attempt_note = if selected_for_attempt.is_empty() {
                        "auto device selection"
                    } else {
                        "explicit device selection"
                    };
                    let reason = if let Some(line) = startup_error {
                        format!("startup error: {}", line)
                    } else if let Some(err) = self.gpu_last_error.clone() {
                        err
                    } else if self.gpu_miner_child.is_none() {
                        "GPU miner process exited during startup".to_string()
                    } else {
                        "No job/hashrate activity detected within startup window".to_string()
                    };
                    failures.push(format!(
                        "{} backend failed during startup ({}, {}, target {}%)\nReason: {}\n{}",
                        backend, attempt_note, tuning_note, utilization_percent, reason, log_tail
                    ));
                    let _ = self.stop_gpu_miner_sync();
                }
            }
        }

        // Check for known issues and provide actionable messages
        let all_failures = failures.join("\n");
        let message = if all_failures.contains("invalid device symbol") {
            // CUDA compute capability mismatch: ethminer 0.18.0 only supports up to ~Compute 7.5.
            // Newer GPUs (RTX 30xx=8.6, 40xx=8.9, 50xx=12.0) are incompatible.
            "GPU mining is not available: your GPU is too new for the bundled miner (ethminer 0.18.0).\n\
             This affects RTX 30-series and newer NVIDIA GPUs.\n\
             Use CPU mining instead, or set CHIRAL_GPU_MINER_PATH to a compatible ethash miner."
                .to_string()
        } else if all_failures.contains("No usable mining devices found") {
            "GPU mining is not available: no compatible GPU detected.\n\
             Make sure your GPU drivers are installed. Use CPU mining as an alternative."
                .to_string()
        } else if failures.is_empty() {
            "GPU miner could not start with any backend".to_string()
        } else {
            format!(
                "GPU miner failed to start with available backends. Details:\n{}",
                failures.join("\n\n")
            )
        };
        self.gpu_last_error = Some(message.clone());
        Err(message)
    }

    pub async fn stop_gpu_mining(&mut self) -> Result<(), String> {
        self.stop_gpu_miner_sync()
    }

    pub async fn get_gpu_mining_status(&mut self) -> Result<GpuMiningStatus, String> {
        self.refresh_gpu_runtime();
        Ok(GpuMiningStatus {
            running: self.gpu_miner_child.is_some(),
            hash_rate: self.gpu_hash_rate,
            active_devices: self.gpu_active_devices.clone(),
            utilization_percent: self.gpu_utilization_percent,
            last_error: self.gpu_last_error.clone(),
        })
    }

    /// Get mining status
    /// Note: eth_hashrate returns 0 for Geth's internal CPU miner (known upstream issue).
    /// We estimate hashrate from block difficulty / block time instead.
    pub async fn get_mining_status(&mut self) -> Result<MiningStatus, String> {
        self.refresh_gpu_runtime();
        let gpu_running = self.gpu_miner_child.is_some();
        let endpoint = self.effective_rpc_endpoint();

        // Batch: eth_mining + eth_hashrate + eth_coinbase + eth_blockNumber in one request
        let mut batch = crate::rpc_client::batch();
        let mining_idx = batch.add("eth_mining", serde_json::json!([]));
        let hashrate_idx = batch.add("eth_hashrate", serde_json::json!([]));
        let coinbase_idx = batch.add("eth_coinbase", serde_json::json!([]));
        let blocknum_idx = batch.add("eth_blockNumber", serde_json::json!([]));

        let results = batch.execute(&endpoint).await?;

        let cpu_mining = results[mining_idx]
            .as_ref()
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mut hash_rate: u64 = if cpu_mining {
            results[hashrate_idx]
                .as_ref()
                .ok()
                .and_then(|v| v.as_str())
                .map(crate::rpc_client::hex_to_u64)
                .unwrap_or(0)
        } else {
            0
        };

        let miner_address = results[coinbase_idx]
            .as_ref()
            .ok()
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let current_block = results[blocknum_idx]
            .as_ref()
            .ok()
            .and_then(|v| v.as_str())
            .map(crate::rpc_client::hex_to_u64)
            .unwrap_or(0);

        // Fallback hash rate estimation from block production
        if cpu_mining && hash_rate == 0 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            if current_block > 0
                && self.last_block > 0
                && current_block > self.last_block
                && self.last_block_time > 0
            {
                let blocks_mined = current_block - self.last_block;
                let elapsed = now.saturating_sub(self.last_block_time);
                if elapsed > 0 {
                    if let Ok(block) = crate::rpc_client::call(
                        &endpoint,
                        "eth_getBlockByNumber",
                        serde_json::json!(["latest", false]),
                    )
                    .await
                    {
                        let difficulty = block
                            .get("difficulty")
                            .and_then(|d| d.as_str())
                            .map(crate::rpc_client::hex_to_u64)
                            .unwrap_or(0);
                        if difficulty > 0 {
                            hash_rate = (blocks_mined as u128 * difficulty as u128
                                / elapsed as u128)
                                as u64;
                        }
                    }
                }
            }

            if current_block > 0 {
                self.last_block = current_block;
                self.last_block_time = now;
            }
        } else if !cpu_mining {
            self.last_block = 0;
            self.last_block_time = 0;
        }

        let mining = cpu_mining || gpu_running;
        if gpu_running {
            hash_rate = self.gpu_hash_rate;
        }

        // Get miner balance — single call using shared client
        let (total_mined_wei, total_mined_chi) = if let Some(ref addr) = miner_address {
            match crate::rpc_client::call(
                &endpoint,
                "eth_getBalance",
                serde_json::json!([addr, "latest"]),
            )
            .await
            {
                Ok(val) => {
                    let wei = crate::rpc_client::hex_to_u128(val.as_str().unwrap_or("0x0"));
                    (wei.to_string(), crate::rpc_client::wei_to_chi(wei))
                }
                Err(_) => ("0".to_string(), 0.0),
            }
        } else {
            ("0".to_string(), 0.0)
        };

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
        let endpoint = self.effective_rpc_endpoint();

        // Batch: get current block + coinbase in one request
        let mut init_batch = crate::rpc_client::batch();
        let block_idx = init_batch.add("eth_blockNumber", serde_json::json!([]));
        let coinbase_idx = init_batch.add("eth_coinbase", serde_json::json!([]));
        let init_results = init_batch.execute(&endpoint).await?;

        let current_block = init_results[block_idx]
            .as_ref()
            .ok()
            .and_then(|v| v.as_str())
            .map(crate::rpc_client::hex_to_u64)
            .unwrap_or(0);
        if current_block == 0 {
            return Ok(Vec::new());
        }

        let miner_address = init_results[coinbase_idx]
            .as_ref()
            .ok()
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();
        if miner_address.is_empty() {
            return Ok(Vec::new());
        }

        let start_block = current_block.saturating_sub(max_blocks);
        let mut mined_blocks = Vec::new();

        // Scan blocks in batches of 50 using JSON-RPC batch requests
        const BATCH_SIZE: u64 = 50;
        let mut cursor = current_block;

        while cursor >= start_block && mined_blocks.len() < 50 {
            let batch_start = cursor.saturating_sub(BATCH_SIZE - 1).max(start_block);
            let block_range: Vec<u64> = (batch_start..=cursor).rev().collect();

            // Build batch request for all blocks in this range
            let payloads: Vec<serde_json::Value> = block_range
                .iter()
                .enumerate()
                .map(|(i, &num)| {
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "eth_getBlockByNumber",
                        "params": [format!("0x{:x}", num), false],
                        "id": i + 1
                    })
                })
                .collect();

            let resp = crate::rpc_client::client()
                .post(&endpoint)
                .json(&payloads)
                .send()
                .await;

            if let Ok(response) = resp {
                if let Ok(results) = response.json::<Vec<serde_json::Value>>().await {
                    for (i, item) in results.iter().enumerate() {
                        if let Some(block) = item.get("result") {
                            let block_miner = block
                                .get("miner")
                                .and_then(|m| m.as_str())
                                .unwrap_or("")
                                .to_lowercase();

                            if block_miner == miner_address {
                                let block_num = block_range.get(i).copied().unwrap_or(0);
                                let timestamp = block
                                    .get("timestamp")
                                    .and_then(|t| t.as_str())
                                    .map(crate::rpc_client::hex_to_u64)
                                    .unwrap_or(0);
                                let difficulty = block
                                    .get("difficulty")
                                    .and_then(|d| d.as_str())
                                    .map(crate::rpc_client::hex_to_u64)
                                    .unwrap_or(0);

                                let reward_wei: u128 = 5_000_000_000_000_000_000;
                                mined_blocks.push(MinedBlock {
                                    block_number: block_num,
                                    timestamp,
                                    reward_wei: reward_wei.to_string(),
                                    reward_chi: crate::rpc_client::wei_to_chi(reward_wei),
                                    difficulty,
                                });

                                if mined_blocks.len() >= 50 {
                                    break;
                                }
                            }
                        }
                    }
                }
            } else {
                break;
            }

            if batch_start <= start_block {
                break;
            }
            cursor = batch_start - 1;
        }

        Ok(mined_blocks)
    }

    /// Set miner address (coinbase) — requires local Geth to be running
    pub async fn set_miner_address(&self, address: &str) -> Result<(), String> {
        if self.child.is_none() {
            return Err("Cannot set miner address: local Geth node is not running".to_string());
        }
        let endpoint = self.effective_rpc_endpoint();
        crate::rpc_client::call(&endpoint, "miner_setEtherbase", serde_json::json!([address]))
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
            assert!(
                endpoint.contains("130.245.173.73"),
                "should be remote even when local flag is set"
            );
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
        assert!(
            alloc.as_object().unwrap().is_empty(),
            "alloc should be empty to match bootstrap node"
        );
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

    #[test]
    fn test_mining_status_zero_hash_rate() {
        let status = MiningStatus {
            mining: false,
            hash_rate: 0,
            miner_address: Some("0xtest".to_string()),
            total_mined_wei: "0".to_string(),
            total_mined_chi: 0.0,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"hashRate\":0"));
        let deser: MiningStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.hash_rate, 0);
    }

    #[test]
    fn test_mining_status_large_total_mined() {
        let status = MiningStatus {
            mining: true,
            hash_rate: 5000,
            miner_address: Some("0xrich".to_string()),
            total_mined_wei: "1000000000000000000000000".to_string(),
            total_mined_chi: 1_000_000.0,
        };
        let json = serde_json::to_string(&status).unwrap();
        let deser: MiningStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.total_mined_chi, 1_000_000.0);
    }

    #[test]
    fn test_mined_block_zero_difficulty() {
        let block = MinedBlock {
            block_number: 1,
            timestamp: 1700000000,
            reward_wei: "5000000000000000000".to_string(),
            reward_chi: 5.0,
            difficulty: 0,
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("\"difficulty\":0"));
        let deser: MinedBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.difficulty, 0);
    }

    #[test]
    fn test_mined_block_max_block_number() {
        let block = MinedBlock {
            block_number: u64::MAX,
            timestamp: 0,
            reward_wei: "0".to_string(),
            reward_chi: 0.0,
            difficulty: 0,
        };
        let json = serde_json::to_string(&block).unwrap();
        let deser: MinedBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.block_number, u64::MAX);
    }

    #[test]
    fn test_mining_status_full_round_trip() {
        let original = MiningStatus {
            mining: true,
            hash_rate: 42000,
            miner_address: Some("0xabcdef1234567890".to_string()),
            total_mined_wei: "25000000000000000000".to_string(),
            total_mined_chi: 25.0,
        };
        let json = serde_json::to_string(&original).unwrap();
        let restored: MiningStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.mining, original.mining);
        assert_eq!(restored.hash_rate, original.hash_rate);
        assert_eq!(restored.miner_address, original.miner_address);
        assert_eq!(restored.total_mined_wei, original.total_mined_wei);
        assert_eq!(restored.total_mined_chi, original.total_mined_chi);
    }

    #[test]
    fn test_gpu_device_serialization_round_trip() {
        let dev = GpuDevice {
            id: "0".to_string(),
            name: "NVIDIA RTX".to_string(),
        };
        let json = serde_json::to_string(&dev).unwrap();
        let restored: GpuDevice = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.id, "0");
        assert_eq!(restored.name, "NVIDIA RTX");
    }

    #[test]
    fn test_parse_gpu_devices_bracket_format() {
        let output = r#"
[0] : NVIDIA GeForce RTX 4090
[1] : NVIDIA GeForce RTX 3080
"#;
        let devices = GethProcess::parse_gpu_devices_from_text(output);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].id, "0");
        assert_eq!(devices[0].name, "NVIDIA GeForce RTX 4090");
        assert_eq!(devices[1].id, "1");
        assert_eq!(devices[1].name, "NVIDIA GeForce RTX 3080");
    }

    #[test]
    fn test_parse_gpu_devices_tabular_format() {
        let output = r#"
ethminer 0.18.0
Build: windows/release/msvc

Id Pci Id Type Name CUDA SM Total Memory
0 01:00.0 Gpu NVIDIA GeForce RTX 4090 8.9 24564 MB
1 02:00.0 Gpu NVIDIA GeForce RTX 3080 8.6 10018 MB
"#;
        let devices = GethProcess::parse_gpu_devices_from_text(output);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].id, "0");
        assert_eq!(devices[0].name, "NVIDIA GeForce RTX 4090");
        assert_eq!(devices[1].id, "1");
        assert_eq!(devices[1].name, "NVIDIA GeForce RTX 3080");
    }

    #[test]
    fn test_parse_gpu_devices_tabular_without_metrics() {
        let output = r#"
Id Pci Id Type Name
0 03:00.0 Gpu AMD Radeon RX 7800 XT
"#;
        let devices = GethProcess::parse_gpu_devices_from_text(output);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, "0");
        assert_eq!(devices[0].name, "AMD Radeon RX 7800 XT");
    }

    #[test]
    fn test_gpu_utilization_clamp() {
        assert_eq!(GethProcess::clamp_gpu_utilization_percent(None), 100);
        assert_eq!(GethProcess::clamp_gpu_utilization_percent(Some(100)), 100);
        assert_eq!(GethProcess::clamp_gpu_utilization_percent(Some(75)), 75);
        assert_eq!(GethProcess::clamp_gpu_utilization_percent(Some(1)), 10);
    }

    #[test]
    fn test_gpu_log_activity_detection() {
        let log = r#"
m 12:00:01|cuda-0  got work package 0xabc123
m 12:00:12|cuda-0  Speed 41.25 Mh/s
"#;
        assert!(GethProcess::gpu_log_indicates_activity(log));
    }

    #[test]
    fn test_gpu_log_fatal_error_detection() {
        let log = "FATAL: unrecognized option '--opencl-global-work'";
        assert!(GethProcess::gpu_log_fatal_error(log).is_some());
    }

    #[test]
    fn test_gpu_mining_capabilities_serialization() {
        let caps = GpuMiningCapabilities {
            supported: true,
            binary_path: Some("/usr/bin/ethminer".to_string()),
            devices: vec![GpuDevice {
                id: "0".to_string(),
                name: "GPU Zero".to_string(),
            }],
            running: false,
            active_devices: vec!["0".to_string()],
            utilization_percent: 90,
            last_error: None,
        };
        let json = serde_json::to_string(&caps).unwrap();
        assert!(json.contains("binaryPath"));
        assert!(json.contains("activeDevices"));
        assert!(json.contains("utilizationPercent"));
        let restored: GpuMiningCapabilities = serde_json::from_str(&json).unwrap();
        assert!(restored.supported);
        assert_eq!(restored.devices.len(), 1);
        assert_eq!(restored.utilization_percent, 90);
    }

    #[test]
    fn test_gpu_mining_status_serialization() {
        let status = GpuMiningStatus {
            running: true,
            hash_rate: 123_000_000,
            active_devices: vec!["0".to_string(), "1".to_string()],
            utilization_percent: 80,
            last_error: None,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("hashRate"));
        let restored: GpuMiningStatus = serde_json::from_str(&json).unwrap();
        assert!(restored.running);
        assert_eq!(restored.active_devices.len(), 2);
        assert_eq!(restored.utilization_percent, 80);
    }

    #[test]
    fn test_mined_block_full_round_trip() {
        let original = MinedBlock {
            block_number: 999,
            timestamp: 1700123456,
            reward_wei: "5000000000000000000".to_string(),
            reward_chi: 5.0,
            difficulty: 131072,
        };
        let json = serde_json::to_string(&original).unwrap();
        let restored: MinedBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.block_number, original.block_number);
        assert_eq!(restored.timestamp, original.timestamp);
        assert_eq!(restored.reward_wei, original.reward_wei);
        assert_eq!(restored.reward_chi, original.reward_chi);
        assert_eq!(restored.difficulty, original.difficulty);
    }

    // ========================================================================
    // detect_log_level tests
    // ========================================================================

    #[test]
    fn test_detect_log_level_error_keyword() {
        assert_eq!(detect_log_level("Something ERROR happened"), "ERROR");
    }

    #[test]
    fn test_detect_log_level_error_case_insensitive() {
        assert_eq!(detect_log_level("operation failed"), "ERROR");
    }

    #[test]
    fn test_detect_log_level_error_emoji() {
        assert_eq!(detect_log_level("something went wrong ❌"), "ERROR");
    }

    #[test]
    fn test_detect_log_level_warn_keyword() {
        assert_eq!(detect_log_level("WARN: low disk space"), "WARN");
    }

    #[test]
    fn test_detect_log_level_warn_case_insensitive() {
        assert_eq!(detect_log_level("this is a warning"), "WARN");
    }

    #[test]
    fn test_detect_log_level_warn_emoji() {
        assert_eq!(detect_log_level("caution ⚠️"), "WARN");
    }

    #[test]
    fn test_detect_log_level_debug_keyword() {
        assert_eq!(detect_log_level("DEBUG: variable state"), "DEBUG");
    }

    #[test]
    fn test_detect_log_level_debug_emoji() {
        assert_eq!(detect_log_level("inspecting 🔍 internal state"), "DEBUG");
    }

    #[test]
    fn test_detect_log_level_info_default() {
        assert_eq!(detect_log_level("Geth started on port 8545"), "INFO");
    }

    #[test]
    fn test_detect_log_level_empty_string() {
        assert_eq!(detect_log_level(""), "INFO");
    }

    #[test]
    fn test_detect_log_level_error_takes_priority_over_warn() {
        // Both ERROR and WARN present — ERROR check runs first
        assert_eq!(detect_log_level("ERROR and WARN together"), "ERROR");
    }

    // ========================================================================
    // detect_log_source tests
    // ========================================================================

    #[test]
    fn test_detect_log_source_mining_keyword() {
        assert_eq!(detect_log_source("mining started on thread 1"), "MINING");
    }

    #[test]
    fn test_detect_log_source_hashrate_keyword() {
        assert_eq!(detect_log_source("current hashrate: 1000"), "MINING");
    }

    #[test]
    fn test_detect_log_source_mining_emoji() {
        assert_eq!(detect_log_source("⛏️ block found"), "MINING");
    }

    #[test]
    fn test_detect_log_source_geth_default() {
        assert_eq!(detect_log_source("peer connected from 10.0.0.1"), "GETH");
    }

    #[test]
    fn test_detect_log_source_empty_string() {
        assert_eq!(detect_log_source(""), "GETH");
    }

    // ========================================================================
    // parse_hash_rate_from_line tests
    // ========================================================================

    #[test]
    fn test_parse_hash_rate_mh() {
        let rate = GethProcess::parse_hash_rate_from_line("Speed 41.25 Mh/s");
        assert_eq!(rate, Some(41_250_000));
    }

    #[test]
    fn test_parse_hash_rate_kh() {
        let rate = GethProcess::parse_hash_rate_from_line("Speed 500 Kh/s");
        assert_eq!(rate, Some(500_000));
    }

    #[test]
    fn test_parse_hash_rate_gh() {
        let rate = GethProcess::parse_hash_rate_from_line("Speed 1.5 Gh/s");
        assert_eq!(rate, Some(1_500_000_000));
    }

    #[test]
    fn test_parse_hash_rate_h() {
        let rate = GethProcess::parse_hash_rate_from_line("Speed 750 H/s");
        assert_eq!(rate, Some(750));
    }

    #[test]
    fn test_parse_hash_rate_no_match() {
        let rate = GethProcess::parse_hash_rate_from_line("connected to peer");
        assert_eq!(rate, None);
    }

    #[test]
    fn test_parse_hash_rate_case_insensitive() {
        let rate = GethProcess::parse_hash_rate_from_line("Speed 10.0 mh/s ok");
        assert_eq!(rate, Some(10_000_000));
    }

    #[test]
    fn test_parse_hash_rate_with_comma_decimal() {
        // European locale: comma as decimal separator
        let rate = GethProcess::parse_hash_rate_from_line("Speed 41,25 Mh/s");
        assert_eq!(rate, Some(41_250_000));
    }

    #[test]
    fn test_parse_hash_rate_zero_returns_none() {
        // Value of 0 H/s should return None (the function checks h > 0)
        let rate = GethProcess::parse_hash_rate_from_line("Speed 0 Mh/s");
        assert_eq!(rate, None);
    }

    // ========================================================================
    // gpu_tuning_args tests
    // ========================================================================

    #[test]
    fn test_gpu_tuning_args_cuda_full_utilization() {
        let args = GethProcess::gpu_tuning_args("cuda", 100);
        assert!(args.is_empty(), "100% should produce no tuning args");
    }

    #[test]
    fn test_gpu_tuning_args_opencl_full_utilization() {
        let args = GethProcess::gpu_tuning_args("opencl", 100);
        assert!(args.is_empty());
    }

    #[test]
    fn test_gpu_tuning_args_cuda_partial() {
        let args = GethProcess::gpu_tuning_args("cuda", 50);
        assert_eq!(args.len(), 4);
        assert_eq!(args[0], "--cuda-grid-size");
        assert_eq!(args[2], "--cuda-streams");
        // 50% < 70% so streams should be 1
        assert_eq!(args[3], "1");
    }

    #[test]
    fn test_gpu_tuning_args_cuda_high_utilization() {
        let args = GethProcess::gpu_tuning_args("cuda", 75);
        assert_eq!(args[2], "--cuda-streams");
        // 75% >= 70% so streams should be 2
        assert_eq!(args[3], "2");
    }

    #[test]
    fn test_gpu_tuning_args_opencl_partial() {
        let args = GethProcess::gpu_tuning_args("opencl", 50);
        assert_eq!(args.len(), 4);
        assert_eq!(args[0], "--opencl-global-work");
        assert_eq!(args[2], "--opencl-local-work");
        // 50% < 70% so local_work should be 64
        assert_eq!(args[3], "64");
    }

    #[test]
    fn test_gpu_tuning_args_opencl_high_utilization() {
        let args = GethProcess::gpu_tuning_args("opencl", 80);
        assert_eq!(args[2], "--opencl-local-work");
        // 80% >= 70% so local_work should be 128
        assert_eq!(args[3], "128");
    }

    #[test]
    fn test_gpu_tuning_args_cuda_minimum_grid() {
        // Very low utilization — grid should be clamped to at least 1024
        let args = GethProcess::gpu_tuning_args("cuda", 10);
        let grid: u32 = args[1].parse().unwrap();
        assert!(grid >= 1_024, "grid {} should be at least 1024", grid);
        assert_eq!(grid % 256, 0, "grid should be aligned to 256");
    }

    #[test]
    fn test_gpu_tuning_args_opencl_minimum_global_work() {
        let args = GethProcess::gpu_tuning_args("opencl", 10);
        let global_work: u32 = args[1].parse().unwrap();
        assert!(
            global_work >= 4_096,
            "global_work {} should be at least 4096",
            global_work
        );
        assert_eq!(global_work % 64, 0, "global_work should be aligned to 64");
    }

    // ========================================================================
    // gpu_log_indicates_activity tests
    // ========================================================================

    #[test]
    fn test_gpu_log_no_activity() {
        let log = "ethminer 0.18.0\nBuild: linux/release/gnu\n";
        assert!(!GethProcess::gpu_log_indicates_activity(log));
    }

    #[test]
    fn test_gpu_log_activity_new_job() {
        assert!(GethProcess::gpu_log_indicates_activity("new job received"));
    }

    #[test]
    fn test_gpu_log_activity_epoch() {
        assert!(GethProcess::gpu_log_indicates_activity(
            "Epoch 400 requires 4.97 GB"
        ));
    }

    #[test]
    fn test_gpu_log_activity_hash_rate_line() {
        assert!(GethProcess::gpu_log_indicates_activity(
            "Speed 42.00 Mh/s"
        ));
    }

    #[test]
    fn test_gpu_log_activity_empty() {
        assert!(!GethProcess::gpu_log_indicates_activity(""));
    }

    // ========================================================================
    // gpu_log_fatal_error tests
    // ========================================================================

    #[test]
    fn test_gpu_log_fatal_error_none() {
        let log = "ethminer started\nnew job\nSpeed 10.0 Mh/s\n";
        assert!(GethProcess::gpu_log_fatal_error(log).is_none());
    }

    #[test]
    fn test_gpu_log_fatal_no_devices() {
        let log = "No usable mining devices found";
        assert!(GethProcess::gpu_log_fatal_error(log).is_some());
    }

    #[test]
    fn test_gpu_log_fatal_cuda_error() {
        let log = "CUDA error: out of memory";
        let err = GethProcess::gpu_log_fatal_error(log);
        assert!(err.is_some());
        assert!(err.unwrap().contains("CUDA"));
    }

    #[test]
    fn test_gpu_log_fatal_connection_refused() {
        let log = "JSON-RPC problem. connection refused";
        assert!(GethProcess::gpu_log_fatal_error(log).is_some());
    }

    #[test]
    fn test_gpu_log_fatal_empty() {
        assert!(GethProcess::gpu_log_fatal_error("").is_none());
    }

    // ========================================================================
    // parse_gpu_devices_from_text edge cases
    // ========================================================================

    #[test]
    fn test_parse_gpu_devices_empty_input() {
        let devices = GethProcess::parse_gpu_devices_from_text("");
        assert!(devices.is_empty());
    }

    #[test]
    fn test_parse_gpu_devices_no_gpu_lines() {
        let output = "ethminer 0.18.0\nBuild: linux/release/gnu\n\n";
        let devices = GethProcess::parse_gpu_devices_from_text(output);
        assert!(devices.is_empty());
    }

    #[test]
    fn test_parse_gpu_devices_bracket_single() {
        let output = "[0] : AMD Radeon RX 580\n";
        let devices = GethProcess::parse_gpu_devices_from_text(output);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, "0");
        assert_eq!(devices[0].name, "AMD Radeon RX 580");
    }

    #[test]
    fn test_parse_gpu_devices_bracket_no_colon() {
        // Some formats omit the colon after the bracket
        let output = "[0] NVIDIA GTX 1080\n";
        let devices = GethProcess::parse_gpu_devices_from_text(output);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].name, "NVIDIA GTX 1080");
    }

    #[test]
    fn test_parse_gpu_devices_bracket_empty_name() {
        // Empty name after bracket should be skipped
        let output = "[0] : \n";
        let devices = GethProcess::parse_gpu_devices_from_text(output);
        assert!(devices.is_empty());
    }

    #[test]
    fn test_parse_gpu_devices_bracket_non_numeric_id() {
        // Non-numeric ID should be skipped
        let output = "[abc] : GPU Name\n";
        let devices = GethProcess::parse_gpu_devices_from_text(output);
        assert!(devices.is_empty());
    }

    // ========================================================================
    // clamp_gpu_utilization_percent additional edge cases
    // ========================================================================

    #[test]
    fn test_gpu_utilization_clamp_at_boundary() {
        assert_eq!(GethProcess::clamp_gpu_utilization_percent(Some(10)), 10);
        assert_eq!(GethProcess::clamp_gpu_utilization_percent(Some(0)), 10);
    }

    #[test]
    fn test_gpu_utilization_clamp_above_max() {
        // Values above 100 get clamped to 100
        assert_eq!(GethProcess::clamp_gpu_utilization_percent(Some(200)), 100);
        assert_eq!(GethProcess::clamp_gpu_utilization_percent(Some(255)), 100);
    }

    // ========================================================================
    // gpu_binary_name test
    // ========================================================================

    #[test]
    fn test_gpu_binary_name() {
        let name = GethProcess::gpu_binary_name();
        if cfg!(target_os = "windows") {
            assert_eq!(name, "ethminer.exe");
        } else {
            assert_eq!(name, "ethminer");
        }
    }

    // ========================================================================
    // GpuMiningCapabilities edge cases
    // ========================================================================

    #[test]
    fn test_gpu_mining_capabilities_unsupported() {
        let caps = GpuMiningCapabilities {
            supported: false,
            binary_path: None,
            devices: vec![],
            running: false,
            active_devices: vec![],
            utilization_percent: 100,
            last_error: Some("No GPU found".to_string()),
        };
        let json = serde_json::to_string(&caps).unwrap();
        let restored: GpuMiningCapabilities = serde_json::from_str(&json).unwrap();
        assert!(!restored.supported);
        assert!(restored.binary_path.is_none());
        assert!(restored.devices.is_empty());
        assert_eq!(restored.last_error.as_deref(), Some("No GPU found"));
    }

    #[test]
    fn test_gpu_mining_capabilities_camel_case() {
        let caps = GpuMiningCapabilities {
            supported: true,
            binary_path: None,
            devices: vec![],
            running: false,
            active_devices: vec![],
            utilization_percent: 50,
            last_error: None,
        };
        let json = serde_json::to_string(&caps).unwrap();
        assert!(json.contains("binaryPath"));
        assert!(!json.contains("binary_path"));
        assert!(json.contains("activeDevices"));
        assert!(!json.contains("active_devices"));
        assert!(json.contains("utilizationPercent"));
        assert!(!json.contains("utilization_percent"));
        assert!(json.contains("lastError"));
        assert!(!json.contains("last_error"));
    }

    // ========================================================================
    // GpuMiningStatus edge cases
    // ========================================================================

    #[test]
    fn test_gpu_mining_status_with_error() {
        let status = GpuMiningStatus {
            running: false,
            hash_rate: 0,
            active_devices: vec![],
            utilization_percent: 100,
            last_error: Some("CUDA out of memory".to_string()),
        };
        let json = serde_json::to_string(&status).unwrap();
        let restored: GpuMiningStatus = serde_json::from_str(&json).unwrap();
        assert!(!restored.running);
        assert_eq!(
            restored.last_error.as_deref(),
            Some("CUDA out of memory")
        );
    }

    #[test]
    fn test_gpu_mining_status_camel_case() {
        let status = GpuMiningStatus {
            running: true,
            hash_rate: 100,
            active_devices: vec!["0".to_string()],
            utilization_percent: 75,
            last_error: None,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("hashRate"));
        assert!(!json.contains("hash_rate"));
        assert!(json.contains("activeDevices"));
        assert!(!json.contains("active_devices"));
        assert!(json.contains("utilizationPercent"));
        assert!(!json.contains("utilization_percent"));
    }

    // ========================================================================
    // GethStatus camelCase verification
    // ========================================================================

    #[test]
    fn test_geth_status_camel_case() {
        let status = GethStatus {
            installed: true,
            running: false,
            local_running: false,
            syncing: true,
            current_block: 50,
            highest_block: 100,
            peer_count: 3,
            chain_id: CHAIN_ID,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("localRunning"));
        assert!(!json.contains("local_running"));
        assert!(json.contains("currentBlock"));
        assert!(!json.contains("current_block"));
        assert!(json.contains("highestBlock"));
        assert!(!json.contains("highest_block"));
        assert!(json.contains("peerCount"));
        assert!(!json.contains("peer_count"));
        assert!(json.contains("chainId"));
        assert!(!json.contains("chain_id"));
    }

    // ========================================================================
    // MiningStatus camelCase verification
    // ========================================================================

    #[test]
    fn test_mining_status_camel_case() {
        let status = MiningStatus {
            mining: true,
            hash_rate: 100,
            miner_address: Some("0x1234".to_string()),
            total_mined_wei: "0".to_string(),
            total_mined_chi: 0.0,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("hashRate"));
        assert!(!json.contains("hash_rate"));
        assert!(json.contains("minerAddress"));
        assert!(!json.contains("miner_address"));
        assert!(json.contains("totalMinedWei"));
        assert!(!json.contains("total_mined_wei"));
        assert!(json.contains("totalMinedChi"));
        assert!(!json.contains("total_mined_chi"));
    }

    // ========================================================================
    // GethStatus deserialization from frontend format
    // ========================================================================

    #[test]
    fn test_geth_status_deserialization_from_frontend() {
        let json = r#"{"installed":true,"running":false,"localRunning":false,"syncing":true,"currentBlock":500,"highestBlock":1000,"peerCount":8,"chainId":98765}"#;
        let status: GethStatus = serde_json::from_str(json).unwrap();
        assert!(status.installed);
        assert!(!status.running);
        assert!(status.syncing);
        assert_eq!(status.current_block, 500);
        assert_eq!(status.highest_block, 1000);
        assert_eq!(status.peer_count, 8);
        assert_eq!(status.chain_id, CHAIN_ID);
    }

    // ========================================================================
    // GpuDevice camelCase and edge cases
    // ========================================================================

    #[test]
    fn test_gpu_device_empty_name() {
        let dev = GpuDevice {
            id: "0".to_string(),
            name: "".to_string(),
        };
        let json = serde_json::to_string(&dev).unwrap();
        let restored: GpuDevice = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.name, "");
    }

    #[test]
    fn test_gpu_device_unicode_name() {
        let dev = GpuDevice {
            id: "0".to_string(),
            name: "GPU \u{00E9}\u{00E8}".to_string(),
        };
        let json = serde_json::to_string(&dev).unwrap();
        let restored: GpuDevice = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.name, dev.name);
    }

    // ========================================================================
    // DownloadProgress deserialization from frontend
    // ========================================================================

    #[test]
    fn test_download_progress_deserialization_from_frontend() {
        let json = r#"{"downloaded":5242880,"total":10485760,"percentage":50.0,"status":"Downloading... 5.0 MB"}"#;
        let progress: DownloadProgress = serde_json::from_str(json).unwrap();
        assert_eq!(progress.downloaded, 5242880);
        assert_eq!(progress.total, 10485760);
        assert_eq!(progress.percentage, 50.0);
        assert!(progress.status.contains("5.0 MB"));
    }

    // ========================================================================
    // Syncing logic tests — verify eth_syncing response parsing
    // ========================================================================

    /// Helper: simulates the syncing parsing logic from get_status
    fn parse_syncing_result(result: serde_json::Value) -> (bool, u64, u64) {
        if result.is_boolean() && !result.as_bool().unwrap_or(false) {
            (false, 0u64, 0u64)
        } else if result.is_null() {
            (false, 0, 0)
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
            let actually_syncing = highest > 0 && current < highest;
            (actually_syncing, current, highest)
        } else {
            (false, 0, 0)
        }
    }

    #[test]
    fn test_syncing_false_boolean() {
        let result = serde_json::json!(false);
        let (syncing, current, highest) = parse_syncing_result(result);
        assert!(!syncing);
        assert_eq!(current, 0);
        assert_eq!(highest, 0);
    }

    #[test]
    fn test_syncing_null_means_not_syncing() {
        let result = serde_json::Value::Null;
        let (syncing, _, _) = parse_syncing_result(result);
        assert!(!syncing);
    }

    #[test]
    fn test_syncing_object_both_zero_means_not_syncing() {
        // Node just started, no sync info yet — should NOT be marked as syncing
        let result = serde_json::json!({
            "currentBlock": "0x0",
            "highestBlock": "0x0",
            "startingBlock": "0x0"
        });
        let (syncing, current, highest) = parse_syncing_result(result);
        assert!(!syncing, "should not be syncing when both blocks are 0");
        assert_eq!(current, 0);
        assert_eq!(highest, 0);
    }

    #[test]
    fn test_syncing_object_current_equals_highest_means_synced() {
        // Node has caught up — not syncing anymore
        let result = serde_json::json!({
            "currentBlock": "0x64",
            "highestBlock": "0x64"
        });
        let (syncing, current, highest) = parse_syncing_result(result);
        assert!(!syncing, "should not be syncing when current == highest");
        assert_eq!(current, 100);
        assert_eq!(highest, 100);
    }

    #[test]
    fn test_syncing_object_behind_means_syncing() {
        // Node is behind — actually syncing
        let result = serde_json::json!({
            "currentBlock": "0x32",
            "highestBlock": "0xc8"
        });
        let (syncing, current, highest) = parse_syncing_result(result);
        assert!(syncing, "should be syncing when current < highest");
        assert_eq!(current, 50);
        assert_eq!(highest, 200);
    }

    #[test]
    fn test_syncing_object_missing_fields_means_not_syncing() {
        // Empty object — treat as not syncing
        let result = serde_json::json!({});
        let (syncing, current, highest) = parse_syncing_result(result);
        assert!(!syncing);
        assert_eq!(current, 0);
        assert_eq!(highest, 0);
    }

    #[test]
    fn test_syncing_true_boolean_treated_as_not_syncing() {
        // eth_syncing returning true as boolean (unusual) — no block info available
        let result = serde_json::json!(true);
        let (syncing, _, _) = parse_syncing_result(result);
        // true boolean doesn't match the `!result.as_bool().unwrap_or(false)` check
        // so it falls through — no object means not syncing
        assert!(!syncing);
    }

    #[test]
    fn test_syncing_one_block_behind() {
        let result = serde_json::json!({
            "currentBlock": "0x63",
            "highestBlock": "0x64"
        });
        let (syncing, current, highest) = parse_syncing_result(result);
        assert!(syncing, "one block behind should be syncing");
        assert_eq!(current, 99);
        assert_eq!(highest, 100);
    }

    #[test]
    fn test_syncing_large_block_numbers() {
        let result = serde_json::json!({
            "currentBlock": "0x4aee7",
            "highestBlock": "0x4b000"
        });
        let (syncing, current, highest) = parse_syncing_result(result);
        assert!(syncing);
        assert_eq!(current, 306919);
        assert_eq!(highest, 307200);
    }

    #[test]
    fn test_syncing_highest_zero_current_nonzero() {
        // Edge case: current block > 0 but highest is 0 (shouldn't happen but be safe)
        let result = serde_json::json!({
            "currentBlock": "0x10",
            "highestBlock": "0x0"
        });
        let (syncing, _, _) = parse_syncing_result(result);
        assert!(!syncing, "should not be syncing when highest is 0");
    }

    // ========================================================================
    // Genesis difficulty and config validation
    // ========================================================================

    #[test]
    fn test_genesis_difficulty_matches_bootstrap() {
        let genesis = GethProcess::get_genesis_json();
        let parsed: serde_json::Value = serde_json::from_str(&genesis).unwrap();
        let difficulty = parsed["difficulty"].as_str().unwrap();
        assert_eq!(
            difficulty, "0x400000",
            "genesis difficulty must match bootstrap node (0x400000)"
        );
    }

    #[test]
    fn test_genesis_difficulty_is_cpu_mineable() {
        let genesis = GethProcess::get_genesis_json();
        let parsed: serde_json::Value = serde_json::from_str(&genesis).unwrap();
        let difficulty = parsed["difficulty"].as_str().unwrap();
        let diff_val =
            u64::from_str_radix(difficulty.trim_start_matches("0x"), 16).unwrap();
        // At 1 MH/s, should find a block in under 60 seconds
        assert!(
            diff_val < 60_000_000,
            "genesis difficulty {} is too high for CPU mining",
            diff_val
        );
    }

    // ========================================================================
    // Mining status serialization
    // ========================================================================

    #[test]
    fn test_mining_status_total_mined_fields() {
        let status = MiningStatus {
            mining: true,
            hash_rate: 500000,
            miner_address: Some("0xabcdef1234567890abcdef1234567890abcdef12".to_string()),
            total_mined_wei: "25000000000000000000".to_string(),
            total_mined_chi: 25.0,
        };
        let json = serde_json::to_string(&status).unwrap();
        let deserialized: MiningStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.total_mined_chi, 25.0);
        assert_eq!(deserialized.total_mined_wei, "25000000000000000000");
        assert_eq!(
            deserialized.miner_address.unwrap(),
            "0xabcdef1234567890abcdef1234567890abcdef12"
        );
    }

    #[test]
    fn test_mining_status_high_hash_rate() {
        let status = MiningStatus {
            mining: true,
            hash_rate: 5_000_000_000, // 5 GH/s
            miner_address: Some("0x1234".to_string()),
            total_mined_wei: "0".to_string(),
            total_mined_chi: 0.0,
        };
        let json = serde_json::to_string(&status).unwrap();
        let deserialized: MiningStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.hash_rate, 5_000_000_000);
    }

    // ========================================================================
    // GethStatus with syncing states
    // ========================================================================

    #[test]
    fn test_geth_status_syncing_state() {
        let status = GethStatus {
            installed: true,
            running: true,
            local_running: true,
            syncing: true,
            current_block: 50,
            highest_block: 200,
            peer_count: 3,
            chain_id: CHAIN_ID,
        };
        let json = serde_json::to_string(&status).unwrap();
        let d: GethStatus = serde_json::from_str(&json).unwrap();
        assert!(d.syncing);
        assert_eq!(d.current_block, 50);
        assert_eq!(d.highest_block, 200);
        assert!(d.local_running);
    }

    #[test]
    fn test_geth_status_synced_state() {
        let status = GethStatus {
            installed: true,
            running: true,
            local_running: true,
            syncing: false,
            current_block: 306919,
            highest_block: 306919,
            peer_count: 1,
            chain_id: CHAIN_ID,
        };
        let json = serde_json::to_string(&status).unwrap();
        let d: GethStatus = serde_json::from_str(&json).unwrap();
        assert!(!d.syncing);
        assert_eq!(d.current_block, d.highest_block);
    }

    // ========================================================================
    // GPU error message detection
    // ========================================================================

    #[test]
    fn test_gpu_error_invalid_device_symbol_detected() {
        let failures = "cuda backend failed\ninvalid device symbol\nsome other text";
        let is_compat_issue = failures.contains("invalid device symbol");
        assert!(is_compat_issue, "should detect CUDA compute capability mismatch");
    }

    #[test]
    fn test_gpu_error_no_usable_devices_detected() {
        let failures = "opencl backend failed\nNo usable mining devices found";
        let is_no_devices = failures.contains("No usable mining devices found");
        assert!(is_no_devices, "should detect no GPU devices");
    }

    #[test]
    fn test_gpu_error_normal_failure_not_misdetected() {
        let failures = "cuda backend failed to spawn: timeout";
        assert!(!failures.contains("invalid device symbol"));
        assert!(!failures.contains("No usable mining devices found"));
    }
}
