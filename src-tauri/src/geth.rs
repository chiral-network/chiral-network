//! Geth lifecycle, mining, and status RPC.
//!
//! One core-geth child process per app instance. This module only exposes
//! the surface the frontend actually uses — download, init, start, stop,
//! mine, query. No bootstrap discovery, no ethminer GPU integration, no
//! genesis-version migration. Those were the three sources of "blocks
//! regress on restart" bugs in the previous implementation and are gone.

use crate::network;
use crate::rpc_client;
use flate2::read::GzDecoder;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

// ============================================================================
// RPC endpoint routing + module state
// ============================================================================

/// True iff our local geth child is currently running. Read by RPC callers
/// so they route to the local node when it's up.
static LOCAL_GETH_RUNNING: AtomicBool = AtomicBool::new(false);

pub fn chain_id() -> u64 {
    network::active().chain_id
}

pub fn network_id() -> u64 {
    network::active().network_id
}

/// Remote RPC fallback for the active network. Overridden by `CHIRAL_RPC_ENDPOINT`.
pub fn rpc_endpoint() -> String {
    std::env::var("CHIRAL_RPC_ENDPOINT")
        .unwrap_or_else(|_| network::active().rpc_fallback.to_string())
}

/// Local geth when it's running, remote fallback otherwise. Correct for
/// processes that *own* the chain state (the CDN server's own chain
/// operations). Use [`wallet_rpc_endpoint`] for anything the desktop
/// user's wallet touches — otherwise a user running local geth that's
/// isolated from the canonical chain will silently mine their own fork
/// and send payments into it, where nobody else can see them.
pub fn effective_rpc_endpoint() -> String {
    if LOCAL_GETH_RUNNING.load(Ordering::Relaxed) {
        "http://127.0.0.1:8545".to_string()
    } else {
        rpc_endpoint()
    }
}

/// Canonical RPC for wallet operations — always the network's public
/// endpoint, never the local node. Ensures balance reads and outbound
/// transactions land on the chain that peers (especially the CDN) can
/// actually observe.
pub fn wallet_rpc_endpoint() -> String {
    rpc_endpoint()
}

/// Ordered fallback list for canonical-chain reads (`eth_getBalance`,
/// `eth_getTransactionByHash`, etc.). The first endpoint is the direct
/// JSON-RPC port; if that's blocked by a firewall or down, the second
/// is the relay's same-origin proxy at `/api/chain/rpc` on port 8080,
/// which forwards to the canonical Geth from the relay's own loopback.
/// `rpc_client::call_with_fallbacks` walks this list in order and
/// returns the first success. **Read paths only** — write paths
/// (`eth_sendRawTransaction`) should still hit a single endpoint to
/// avoid double-broadcast races.
pub fn wallet_rpc_endpoints() -> Vec<String> {
    let primary = rpc_endpoint();
    // Derive the proxy URL from the primary by swapping the JSON-RPC
    // port for 8080 (where the gateway runs) and appending the proxy
    // path. If the user has overridden `CHIRAL_RPC_ENDPOINT` to a
    // non-standard host, we still try the same host's :8080/api/chain/rpc
    // — relay deployments co-locate Geth and the gateway.
    let proxy = derive_proxy_url(&primary);
    if let Some(p) = proxy {
        if p != primary {
            return vec![primary, p];
        }
    }
    vec![primary]
}

fn derive_proxy_url(primary: &str) -> Option<String> {
    // primary is something like http://130.245.173.73:8545. We want
    // http://130.245.173.73:8080/api/chain/rpc.
    let stripped = primary.trim_end_matches('/');
    let scheme_end = stripped.find("://")? + 3;
    let host_end = stripped[scheme_end..]
        .find(|c: char| c == ':' || c == '/')
        .map(|i| scheme_end + i)
        .unwrap_or_else(|| stripped.len());
    let scheme = &stripped[..scheme_end];
    let host = &stripped[scheme_end..host_end];
    Some(format!("{}{}:8080/api/chain/rpc", scheme, host))
}

#[cfg(test)]
mod endpoint_tests {
    use super::*;

    #[test]
    fn proxy_url_derives_from_8545_default() {
        assert_eq!(
            derive_proxy_url("http://130.245.173.73:8545"),
            Some("http://130.245.173.73:8080/api/chain/rpc".to_string())
        );
    }

    #[test]
    fn proxy_url_handles_trailing_slash() {
        assert_eq!(
            derive_proxy_url("http://example.com:8545/"),
            Some("http://example.com:8080/api/chain/rpc".to_string())
        );
    }

    #[test]
    fn proxy_url_handles_no_port() {
        assert_eq!(
            derive_proxy_url("https://example.com"),
            Some("https://example.com:8080/api/chain/rpc".to_string())
        );
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
pub struct MinedBlock {
    pub block_number: u64,
    pub timestamp: u64,
    pub reward_wei: String,
    pub reward_chi: f64,
    pub difficulty: u64,
}

// GPU mining lives in the `geth_gpu` module. Re-exported here so the
// existing import surface (`crate::geth::GpuDevice` etc.) keeps working.
pub use crate::geth_gpu::{GpuDevice, GpuMiningCapabilities, GpuMiningStatus};

// ============================================================================
// Downloader
// ============================================================================

pub struct GethDownloader { base_dir: PathBuf }

impl GethDownloader {
    pub fn new() -> Self {
        let base_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        Self { base_dir }
    }

    pub fn bin_dir(&self) -> PathBuf { self.base_dir.join("bin") }

    pub fn geth_path(&self) -> PathBuf {
        self.bin_dir().join(if cfg!(windows) { "geth.exe" } else { "geth" })
    }

    pub fn is_geth_installed(&self) -> bool { self.geth_path().exists() }

    fn download_url() -> Result<String, String> {
        const BASE: &str = "https://github.com/etclabscore/core-geth/releases/download/v1.12.20";
        let file = match (std::env::consts::OS, std::env::consts::ARCH) {
            ("macos", _)          => "core-geth-osx-v1.12.20.zip",
            ("linux", "x86_64")   => "core-geth-linux-v1.12.20.zip",
            ("linux", "aarch64")  => "core-geth-arm64-v1.12.20.zip",
            ("windows", "x86_64") => "core-geth-win64-v1.12.20.zip",
            (os, arch) => return Err(format!("Unsupported platform: {os} {arch}")),
        };
        Ok(format!("{BASE}/{file}"))
    }

    pub async fn download_geth<F>(&self, on_progress: F) -> Result<(), String>
    where F: Fn(DownloadProgress) + Send + 'static,
    {
        if self.is_geth_installed() {
            on_progress(DownloadProgress { downloaded: 0, total: 0, percentage: 100.0, status: "Geth already installed".into() });
            return Ok(());
        }
        let url = Self::download_url()?;
        fs::create_dir_all(self.bin_dir()).map_err(|e| format!("mkdir bin/: {e}"))?;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| format!("http client: {e}"))?;
        let resp = client.get(&url).send().await.map_err(|e| format!("GET {url}: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("download failed: HTTP {}", resp.status()));
        }
        let total = resp.content_length().unwrap_or(0);

        let mut bytes = Vec::new();
        let mut downloaded = 0u64;
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("read chunk: {e}"))?;
            downloaded += chunk.len() as u64;
            bytes.extend_from_slice(&chunk);
            let percentage = if total > 0 { (downloaded as f32 / total as f32) * 100.0 } else { 0.0 };
            on_progress(DownloadProgress {
                downloaded, total, percentage,
                status: format!("Downloading... {:.1} MB", downloaded as f32 / 1_048_576.0),
            });
        }

        on_progress(DownloadProgress { downloaded, total, percentage: 100.0, status: "Extracting...".into() });
        Self::extract_geth_binary(&bytes, &self.bin_dir())?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let p = self.geth_path();
            if p.exists() {
                let mut perms = fs::metadata(&p).map_err(|e| format!("metadata: {e}"))?.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&p, perms).map_err(|e| format!("chmod: {e}"))?;
            }
        }

        on_progress(DownloadProgress { downloaded: total, total, percentage: 100.0, status: "Installation complete".into() });
        Ok(())
    }

    fn extract_geth_binary(data: &[u8], out_dir: &Path) -> Result<(), String> {
        let target = if cfg!(windows) { "geth.exe" } else { "geth" };
        // core-geth releases ship as .zip for every platform as of v1.12.x.
        if let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(data)) {
            for i in 0..archive.len() {
                let mut entry = archive.by_index(i).map_err(|e| format!("zip entry {i}: {e}"))?;
                if entry.name().rsplit('/').next() == Some(target) {
                    let mut out = fs::File::create(out_dir.join(target))
                        .map_err(|e| format!("create {target}: {e}"))?;
                    std::io::copy(&mut entry, &mut out).map_err(|e| format!("copy: {e}"))?;
                    return Ok(());
                }
            }
            return Err(format!("{target} not found in zip"));
        }
        // Legacy tar.gz fallback, just in case the release format changes.
        let mut archive = tar::Archive::new(GzDecoder::new(Cursor::new(data)));
        for entry in archive.entries().map_err(|e| format!("tar: {e}"))? {
            let mut entry = entry.map_err(|e| format!("tar entry: {e}"))?;
            let path = entry.path().map_err(|e| format!("tar path: {e}"))?;
            if path.file_name().and_then(|s| s.to_str()) == Some(target) {
                entry.unpack(out_dir.join(target)).map_err(|e| format!("unpack: {e}"))?;
                return Ok(());
            }
        }
        Err(format!("{target} not found in archive"))
    }
}

impl Default for GethDownloader {
    fn default() -> Self { Self::new() }
}

// ============================================================================
// Process
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
        let p = GethProcess {
            child: None,
            data_dir,
            downloader: GethDownloader::new(),
            miner_address: None,
        };
        // One-time sweep at startup: if a previous app crashed with geth
        // still running, kill that orphan and remove the stale LevelDB LOCK
        // files it left behind. Otherwise the next start would fail with
        // "another process is using this datadir".
        p.cleanup_previous_run();
        p
    }

    pub fn is_installed(&self) -> bool { self.downloader.is_geth_installed() }
    pub fn is_running(&self) -> bool { self.child.is_some() }
    pub fn geth_path(&self) -> PathBuf { self.downloader.geth_path() }
    pub fn effective_rpc_endpoint(&self) -> String { effective_rpc_endpoint() }

    /// Kill any orphan geth from a crashed previous session and wipe stale
    /// lock files. No `fuser` racing — the PID file plus /proc comm check
    /// is precise enough and doesn't risk killing an unrelated process.
    fn cleanup_previous_run(&self) {
        let pid_file = self.data_dir.join("geth.pid");
        if let Ok(pid_str) = fs::read_to_string(&pid_file) {
            let _ = fs::remove_file(&pid_file);
            if let Ok(pid) = pid_str.trim().parse::<u32>() {
                #[cfg(unix)]
                if fs::read_to_string(format!("/proc/{pid}/comm"))
                    .map(|c| c.trim().contains("geth"))
                    .unwrap_or(false)
                {
                    let _ = Command::new("kill").arg(pid.to_string()).status();
                    std::thread::sleep(std::time::Duration::from_millis(800));
                    let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
                }
                #[cfg(windows)]
                { let _ = Command::new("taskkill").args(["/F", "/PID", &pid.to_string()]).status(); }
            }
        }
        Self::wipe_lock_files(&self.data_dir);
    }

    fn wipe_lock_files(dir: &Path) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                Self::wipe_lock_files(&path);
            } else if path.file_name().and_then(|n| n.to_str()) == Some("LOCK") {
                let _ = fs::remove_file(&path);
            }
        }
    }

    /// `geth init genesis.json` exactly once per chain. If the chaindata
    /// directory already exists we never reinitialize — the previous
    /// implementation's version-marker migration logic was a source of
    /// mid-run wipes that caused "block height regression" reports.
    fn init_genesis_if_needed(&self) -> Result<(), String> {
        if self.data_dir.join("geth").join("chaindata").exists() {
            return Ok(());
        }
        fs::create_dir_all(&self.data_dir).map_err(|e| format!("mkdir datadir: {e}"))?;
        let genesis_path = self.data_dir.join("genesis.json");
        fs::write(&genesis_path, network::genesis_json(network::active()))
            .map_err(|e| format!("write genesis: {e}"))?;
        let out = Command::new(self.geth_path())
            .args(["--datadir"]).arg(&self.data_dir)
            .args(["init"]).arg(&genesis_path)
            .output()
            .map_err(|e| format!("geth init: {e}"))?;
        if !out.status.success() {
            return Err(format!("geth init failed: {}", String::from_utf8_lossy(&out.stderr)));
        }
        Ok(())
    }

    pub async fn start(&mut self, miner_address: Option<&str>) -> Result<(), String> {
        if self.child.is_some() {
            return Err("Geth is already running".into());
        }
        if !self.is_installed() {
            return Err("Geth binary not installed. Download it first.".into());
        }
        self.cleanup_previous_run();
        self.init_genesis_if_needed()?;
        self.miner_address = miner_address.map(str::to_owned);

        let cfg = network::active();
        let log_path = self.data_dir.join("geth.log");
        if let Some(parent) = log_path.parent() { let _ = fs::create_dir_all(parent); }
        let log_file = OpenOptions::new().create(true).write(true).truncate(true)
            .open(&log_path).map_err(|e| format!("open log file: {e}"))?;
        let log_clone = log_file.try_clone().map_err(|e| format!("clone log: {e}"))?;

        // Bind address for the HTTP RPC. Default 127.0.0.1 (safe for desktop
        // users), overridable to 0.0.0.0 for server operators running the
        // chain's public RPC endpoint.
        let http_addr = std::env::var("CHIRAL_GETH_HTTP_ADDR")
            .unwrap_or_else(|_| "127.0.0.1".to_string());
        let mut cmd = Command::new(self.geth_path());
        cmd.args(["--datadir"]).arg(&self.data_dir)
            .args(["--networkid", &network_id().to_string()])
            .args(["--http", "--http.addr", &http_addr, "--http.port", "8545"])
            // `admin` is intentionally absent here. It exposes
            // `admin_stopRPC` (deprecated alias of `admin_stopHTTP`)
            // which any unauthenticated HTTP caller could use to
            // shut down the RPC server itself — observed in the wild
            // on the canonical relay 2026-04-28: a remote IP issued
            // admin_stopRPC and the HTTP server stayed down for two
            // days, breaking every client's wallet balance. Admin
            // calls remain available over the IPC socket for
            // operators who need them.
            .args(["--http.api", "eth,net,web3,personal,debug,miner,txpool"])
            .args(["--http.corsdomain", "*"])
            // Full sync + archive GC is the key to not regressing block height
            // on restart. Snap/fast sync can roll back unfinalized state; that
            // was the root cause of the bug we're rebuilding to fix.
            .args(["--syncmode", "full", "--gcmode", "archive"])
            .args(["--cache", "256"])
            .args(["--port", "30303", "--maxpeers", "25"])
            .args(["--miner.gasprice", "0", "--txpool.pricelimit", "0"])
            .stdout(Stdio::from(log_clone))
            .stderr(Stdio::from(log_file));

        if cfg.geth_bootstrap_enode.is_empty() {
            cmd.arg("--nodiscover"); // solo mining, no peer discovery
        } else {
            cmd.args(["--bootnodes", cfg.geth_bootstrap_enode]);
        }

        if let Some(addr) = miner_address {
            cmd.args(["--miner.etherbase", addr, "--mine", "--miner.threads", "1"]);
        }

        let child = cmd.spawn().map_err(|e| format!("spawn geth: {e}"))?;
        let _ = fs::write(self.data_dir.join("geth.pid"), child.id().to_string());
        self.child = Some(child);
        LOCAL_GETH_RUNNING.store(true, Ordering::Relaxed);

        // Brief startup wait so a crash (bad genesis, port collision) surfaces
        // here rather than as a mysterious RPC failure on the next call.
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        if let Some(ref mut child) = self.child {
            if let Ok(Some(status)) = child.try_wait() {
                self.child = None;
                LOCAL_GETH_RUNNING.store(false, Ordering::Relaxed);
                let tail = fs::read_to_string(&log_path).unwrap_or_default()
                    .lines().rev().take(20).collect::<Vec<_>>()
                    .into_iter().rev().collect::<Vec<_>>().join("\n");
                return Err(format!("Geth exited on startup ({status}).\n{tail}"));
            }
        }
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), String> {
        let Some(mut child) = self.child.take() else { return Ok(()) };
        LOCAL_GETH_RUNNING.store(false, Ordering::Relaxed);
        let pid = child.id();
        #[cfg(unix)]   let _ = Command::new("kill").arg(pid.to_string()).status();
        #[cfg(windows)] let _ = Command::new("taskkill").args(["/PID", &pid.to_string()]).status();
        for _ in 0..6 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            if let Ok(Some(_)) = child.try_wait() {
                let _ = fs::remove_file(self.data_dir.join("geth.pid"));
                return Ok(());
            }
        }
        // SIGTERM ignored for 3s → force kill.
        #[cfg(unix)]    let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
        #[cfg(windows)] let _ = Command::new("taskkill").args(["/F", "/PID", &pid.to_string()]).status();
        let _ = child.wait();
        let _ = fs::remove_file(self.data_dir.join("geth.pid"));
        Ok(())
    }

    /// Wipe the local chaindata so the next `start()` does a fresh
    /// `geth init` and re-syncs from the canonical bootnode. Used to
    /// recover from "I mined on a private fork" — the local node has
    /// accumulated more cumulative work than what canonical has done in
    /// the same wall-clock window, so Ethash won't switch back on its
    /// own. Only the chaindata is removed; `nodekey` and any wallet
    /// keystore stay put.
    ///
    /// Geth must be stopped before calling. Mining rewards on the
    /// pre-reset local fork are unrecoverable — they were never on the
    /// canonical chain.
    pub fn reset_chain(&mut self) -> Result<(), String> {
        if self.child.is_some() {
            return Err("Stop Geth before resetting the local chain".into());
        }
        let geth_dir = self.data_dir.join("geth");
        for sub in ["chaindata", "lightchaindata", "triecache", "ethash"] {
            let p = geth_dir.join(sub);
            if p.exists() {
                fs::remove_dir_all(&p)
                    .map_err(|e| format!("remove {}: {}", p.display(), e))?;
            }
        }
        // The genesis.json file at the datadir root is regenerated on
        // next start by init_genesis_if_needed, so wipe it too — leaving
        // a stale copy from an old preset would be confusing if the
        // operator switched networks between resets.
        let genesis = self.data_dir.join("genesis.json");
        if genesis.exists() {
            let _ = fs::remove_file(&genesis);
        }
        Ok(())
    }

    /// Fast kill for app shutdown — no graceful wait.
    pub fn stop_fast(&mut self) {
        if let Some(mut child) = self.child.take() {
            LOCAL_GETH_RUNNING.store(false, Ordering::Relaxed);
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(self.data_dir.join("geth.pid"));
        }
    }

    /// Batched status RPC — a single HTTP round-trip covers block number,
    /// peer count, and sync state instead of three serial calls. Matters
    /// when the Mining/Diagnostics page polls every second.
    pub async fn get_status(&self) -> Result<GethStatus, String> {
        let installed = self.is_installed();
        if !self.is_running() {
            return Ok(GethStatus {
                installed, running: false, local_running: false, syncing: false,
                current_block: 0, highest_block: 0, peer_count: 0, chain_id: chain_id(),
            });
        }
        let endpoint = self.effective_rpc_endpoint();
        let mut batch = rpc_client::batch();
        batch.add("eth_blockNumber", serde_json::json!([]));
        batch.add("net_peerCount",   serde_json::json!([]));
        batch.add("eth_syncing",     serde_json::json!([]));
        let results = batch.execute(&endpoint).await?;

        let current_block = batch_hex_u64(results.first(), "eth_blockNumber")?;
        let peer_count = batch_hex_u64(results.get(1), "net_peerCount")? as u32;
        let (syncing, highest_block) = parse_syncing(results.get(2), current_block)?;

        Ok(GethStatus {
            installed, running: true, local_running: true,
            syncing, current_block, highest_block, peer_count, chain_id: chain_id(),
        })
    }

    pub async fn start_mining(&self, threads: u32) -> Result<(), String> {
        if !self.is_running() { return Err("Geth is not running".into()); }
        rpc_client::call(&self.effective_rpc_endpoint(), "miner_start",
            serde_json::json!([threads.max(1) as u64])).await.map(|_| ())
    }

    pub async fn stop_mining(&self) -> Result<(), String> {
        if !self.is_running() { return Err("Geth is not running".into()); }
        rpc_client::call(&self.effective_rpc_endpoint(), "miner_stop", serde_json::json!([]))
            .await.map(|_| ())
    }

    /// Batched mining-status RPC — mining flag, hashrate, coinbase, and
    /// coinbase balance in one HTTP round-trip.
    pub async fn get_mining_status(&self) -> Result<MiningStatus, String> {
        if !self.is_running() {
            return Ok(MiningStatus {
                mining: false, hash_rate: 0,
                miner_address: self.miner_address.clone(),
                total_mined_wei: "0".into(), total_mined_chi: 0.0,
            });
        }
        let endpoint = self.effective_rpc_endpoint();
        let mut batch = rpc_client::batch();
        batch.add("eth_mining",   serde_json::json!([]));
        batch.add("eth_hashrate", serde_json::json!([]));
        batch.add("eth_coinbase", serde_json::json!([]));
        let results = batch.execute(&endpoint).await?;

        let mining = results.first()
            .and_then(|r| r.as_ref().ok()).and_then(|v| v.as_bool()).unwrap_or(false);
        let hash_rate = batch_hex_u64(results.get(1), "eth_hashrate")?;
        let coinbase = results.get(2)
            .and_then(|r| r.as_ref().ok())
            .and_then(|v| v.as_str().map(str::to_owned))
            .or_else(|| self.miner_address.clone());

        // Coinbase balance = approximate total-mined display. Cheap and
        // close enough for the UI; doesn't account for gas/transfers out.
        let (total_mined_wei, total_mined_chi) = if let Some(ref addr) = coinbase {
            match rpc_client::call(&endpoint, "eth_getBalance",
                serde_json::json!([addr, "latest"])).await
            {
                Ok(v) => {
                    let wei = value_hex_u128(&v, "eth_getBalance")?;
                    (wei.to_string(), wei as f64 / 1e18)
                }
                Err(_) => ("0".into(), 0.0),
            }
        } else { ("0".into(), 0.0) };

        Ok(MiningStatus { mining, hash_rate, miner_address: coinbase, total_mined_wei, total_mined_chi })
    }

    pub async fn set_miner_address(&mut self, address: &str) -> Result<(), String> {
        self.miner_address = Some(address.to_owned());
        if !self.is_running() { return Ok(()); }
        rpc_client::call(&self.effective_rpc_endpoint(), "miner_setEtherbase",
            serde_json::json!([address])).await.map(|_| ())
    }

    /// Blocks mined by our coinbase in the last `max_blocks` heights,
    /// newest first. One eth_getBlockByNumber per height. Capped at 500
    /// so this doesn't become a thundering herd on long-lived chains.
    pub async fn get_mined_blocks(&self, max_blocks: u64) -> Result<Vec<MinedBlock>, String> {
        if !self.is_running() { return Ok(Vec::new()); }
        let coinbase = self.miner_address.as_deref().unwrap_or("").to_lowercase();
        if coinbase.is_empty() { return Ok(Vec::new()); }
        let endpoint = self.effective_rpc_endpoint();
        let head = match rpc_client::call(&endpoint, "eth_blockNumber", serde_json::json!([])).await {
            Ok(v) => value_hex_u64(&v, "eth_blockNumber")?,
            Err(_) => 0,
        };
        if head == 0 { return Ok(Vec::new()); }

        let scan = max_blocks.min(500);
        let mut out = Vec::new();
        for offset in 0..scan {
            let Some(n) = head.checked_sub(offset) else { break };
            let block = rpc_client::call(&endpoint, "eth_getBlockByNumber",
                serde_json::json!([format!("0x{n:x}"), false])).await.ok();
            let Some(block) = block.filter(|v| !v.is_null()) else { continue };
            if block.get("miner").and_then(|v| v.as_str()).unwrap_or("").to_lowercase() != coinbase {
                continue;
            }
            out.push(MinedBlock {
                block_number: n,
                timestamp: field_hex_u64(&block, "timestamp", "eth_getBlockByNumber")?,
                reward_wei: "5000000000000000000".into(),
                reward_chi: 5.0,
                difficulty: field_hex_u64(&block, "difficulty", "eth_getBlockByNumber")?,
            });
            if out.len() as u64 >= max_blocks { break; }
        }
        Ok(out)
    }

    // GPU mining lives in the `geth_gpu` module now. The Tauri command
    // handlers in lib.rs route to AppState.gpu_miner directly.
}

impl Default for GethProcess {
    fn default() -> Self { Self::new() }
}

// ============================================================================
// Helpers
// ============================================================================

fn value_hex_str<'a>(value: &'a serde_json::Value, context: &str) -> Result<&'a str, String> {
    value
        .as_str()
        .ok_or_else(|| format!("{context} returned a non-string hex value: {value}"))
}

fn value_hex_u64(value: &serde_json::Value, context: &str) -> Result<u64, String> {
    rpc_client::hex_to_u64(value_hex_str(value, context)?).map_err(|e| format!("{context}: {e}"))
}

fn value_hex_u128(value: &serde_json::Value, context: &str) -> Result<u128, String> {
    rpc_client::hex_to_u128(value_hex_str(value, context)?).map_err(|e| format!("{context}: {e}"))
}

fn field_hex_u64(value: &serde_json::Value, field: &str, context: &str) -> Result<u64, String> {
    let hex = value
        .get(field)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("{context} missing string `{field}` field"))?;
    rpc_client::hex_to_u64(hex).map_err(|e| format!("{context} {field}: {e}"))
}

/// Extract a hex-encoded u64 from the Nth result of a batch call.
fn batch_hex_u64(r: Option<&Result<serde_json::Value, String>>, context: &str) -> Result<u64, String> {
    let value = r
        .ok_or_else(|| format!("{context} missing batch result"))?
        .as_ref()
        .map_err(|e| format!("{context} RPC failed: {e}"))?;
    value_hex_u64(value, context)
}

/// Parse the polymorphic `eth_syncing` response. Returns (is_syncing, highest_block).
fn parse_syncing(
    r: Option<&Result<serde_json::Value, String>>,
    current_block: u64,
) -> Result<(bool, u64), String> {
    let Some(Ok(v)) = r else { return Ok((false, current_block)); };
    if let Some(b) = v.as_bool() { return Ok((b, current_block)); }
    let Some(obj) = v.as_object() else { return Ok((false, current_block)); };
    let current = match obj.get("currentBlock") {
        Some(value) => value_hex_u64(value, "eth_syncing currentBlock")?,
        None => current_block,
    };
    let highest = match obj.get("highestBlock") {
        Some(value) => value_hex_u64(value, "eth_syncing highestBlock")?,
        None => current_block,
    };
    Ok((highest > current, highest.max(current)))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freshnet_is_first_preset_and_has_unique_chain_id() {
        assert_eq!(crate::network::FRESHNET.chain_id, 98763);
        assert_ne!(crate::network::FRESHNET.chain_id, crate::network::TESTNET.chain_id);
        assert_eq!(crate::network::ALL[0].chain_id, crate::network::FRESHNET.chain_id);
    }

    #[test]
    fn network_id_matches_chain_id() {
        assert_eq!(network_id(), chain_id());
    }

    #[test]
    fn effective_endpoint_routes_to_local_when_running() {
        LOCAL_GETH_RUNNING.store(true, Ordering::Relaxed);
        assert_eq!(effective_rpc_endpoint(), "http://127.0.0.1:8545");
        LOCAL_GETH_RUNNING.store(false, Ordering::Relaxed);
    }

    #[test]
    fn parse_syncing_handles_all_three_shapes() {
        let false_ = Ok(serde_json::json!(false));
        let true_ = Ok(serde_json::json!(true));
        let obj_behind = Ok(serde_json::json!({"currentBlock": "0x5", "highestBlock": "0xa"}));
        let obj_caught_up = Ok(serde_json::json!({"currentBlock": "0xa", "highestBlock": "0xa"}));

        assert_eq!(parse_syncing(Some(&false_), 100).unwrap(), (false, 100));
        assert_eq!(parse_syncing(Some(&true_), 100).unwrap(), (true, 100));
        assert_eq!(parse_syncing(Some(&obj_behind), 5).unwrap(), (true, 10));
        assert_eq!(parse_syncing(Some(&obj_caught_up), 10).unwrap(), (false, 10));
    }

    #[test]
    fn parse_syncing_rejects_malformed_hex() {
        let malformed =
            Ok(serde_json::json!({"currentBlock": "0xnot-hex", "highestBlock": "0xa"}));

        assert!(parse_syncing(Some(&malformed), 5).is_err());
    }

    #[test]
    fn mining_status_serializes_camel_case() {
        let s = serde_json::to_string(&MiningStatus {
            mining: true, hash_rate: 42, miner_address: Some("0xabc".into()),
            total_mined_wei: "5".into(), total_mined_chi: 5.0,
        }).unwrap();
        assert!(s.contains("\"hashRate\":42"));
        assert!(s.contains("\"minerAddress\""));
        assert!(s.contains("\"totalMinedWei\""));
    }
}
