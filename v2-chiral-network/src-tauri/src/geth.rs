//! Geth Management Module for Chiral Network
//!
//! This module handles:
//! - Downloading Core-Geth binary
//! - Starting/stopping Geth process
//! - Genesis initialization
//! - RPC communication

use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use futures_util::StreamExt;

// ============================================================================
// Configuration
// ============================================================================

/// Chiral Network chain ID
pub const CHAIN_ID: u64 = 98765;

/// Network ID (same as chain ID for our network)
pub const NETWORK_ID: u64 = 98765;

/// Default RPC endpoint
pub const RPC_ENDPOINT: &str = "http://127.0.0.1:8545";

/// Bootstrap nodes for Chiral Network
const BOOTSTRAP_NODES: &[&str] = &[
    "enode://ae987db6399b50addb75d7822bfad9b4092fbfd79cbfe97e6864b1f17d3e8fcd8e9e190ad109572c1439230fa688a9837e58f0b1ad7c0dc2bc6e4ab328f3991e@130.245.173.105:30303",
    "enode://b3ead5f07d0dbeda56023435a7c05877d67b055df3a8bf18f3d5f7c56873495cd4de5cf031ae9052827c043c12f1d30704088c79fb539c96834bfa74b78bf80b@20.85.124.187:30303",
];

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
}

impl GethProcess {
    pub fn new() -> Self {
        let data_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("chiral-network")
            .join("geth");

        GethProcess {
            child: None,
            data_dir,
            downloader: GethDownloader::new(),
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

    /// Get the genesis.json content for Chiral Network
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
            "difficulty": "0x400",
            "gasLimit": "0x1C9C380",
            "alloc": {}
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

        // Check if blockchain needs initialization
        let chaindata_path = self.data_dir.join("geth").join("chaindata");
        if !chaindata_path.exists() {
            println!("Initializing blockchain...");
            self.init_genesis()?;
        }

        let geth_path = self.geth_path();
        let bootstrap_nodes = BOOTSTRAP_NODES.join(",");

        let mut cmd = Command::new(&geth_path);
        cmd.arg("--datadir")
            .arg(&self.data_dir)
            .arg("--networkid")
            .arg(NETWORK_ID.to_string())
            .arg("--bootnodes")
            .arg(&bootstrap_nodes)
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
            .arg("snap")
            .arg("--cache")
            .arg("1024")
            .arg("--maxpeers")
            .arg("50")
            .arg("--port")
            .arg("30303")
            .arg("--nat")
            .arg("any")
            .arg("--miner.gasprice")
            .arg("0")
            .arg("--txpool.pricelimit")
            .arg("0");

        // Set miner address if provided
        if let Some(addr) = miner_address {
            cmd.arg("--miner.etherbase").arg(addr);
        }

        // Create log file
        let log_path = self.data_dir.join("geth.log");
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

        println!("✅ Geth started");
        println!("   Logs: {}", log_path.display());
        println!("   RPC: {}", RPC_ENDPOINT);

        // Wait a moment for Geth to start
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        Ok(())
    }

    /// Stop Geth process
    pub fn stop(&mut self) -> Result<(), String> {
        if let Some(mut child) = self.child.take() {
            child.kill().map_err(|e| format!("Failed to stop geth: {}", e))?;
            println!("✅ Geth stopped");
        }
        Ok(())
    }

    /// Get current Geth status via RPC
    pub async fn get_status(&self) -> Result<GethStatus, String> {
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
            running: self.child.is_some() || chain_id > 0, // If we can query chain ID, it's running
            syncing,
            current_block: if syncing { current_block } else { block_number },
            highest_block: if syncing { highest_block } else { block_number },
            peer_count,
            chain_id,
        })
    }

    /// Start mining
    pub async fn start_mining(&self, threads: u32) -> Result<(), String> {
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
    pub async fn get_mining_status(&self) -> Result<MiningStatus, String> {
        let client = reqwest::Client::new();

        let mining = match self.rpc_call(&client, "eth_mining", serde_json::json!([])).await {
            Ok(result) => result.as_bool().unwrap_or(false),
            Err(_) => false,
        };

        let hash_rate = match self.rpc_call(&client, "eth_hashrate", serde_json::json!([])).await {
            Ok(result) => {
                let hex = result.as_str().unwrap_or("0x0");
                u64::from_str_radix(hex.trim_start_matches("0x"), 16).unwrap_or(0)
            }
            Err(_) => 0,
        };

        let miner_address = match self.rpc_call(&client, "eth_coinbase", serde_json::json!([])).await
        {
            Ok(result) => result.as_str().map(|s| s.to_string()),
            Err(_) => None,
        };

        Ok(MiningStatus {
            mining,
            hash_rate,
            miner_address,
        })
    }

    /// Set miner address (coinbase)
    pub async fn set_miner_address(&self, address: &str) -> Result<(), String> {
        let client = reqwest::Client::new();
        self.rpc_call(&client, "miner_setEtherbase", serde_json::json!([address]))
            .await
            .map(|_| ())
    }

    /// Make an RPC call to Geth
    async fn rpc_call(
        &self,
        client: &reqwest::Client,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1
        });

        let response = client
            .post(RPC_ENDPOINT)
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
