use ethers::prelude::*;
use rand::rngs::OsRng;
use secp256k1::{PublicKey, Secp256k1, SecretKey};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha3::{Digest, Keccak256};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

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

    pub fn is_running(&self) -> bool {
        // First check if we have a tracked child process
        if self.child.is_some() {
            return true;
        }

        // Also check if geth is actually running on port 8545
        // This handles cases where the app restarted but geth is still running
        if let Ok(response) = std::process::Command::new("curl")
            .arg("-s")
            .arg("-X")
            .arg("POST")
            .arg("-H")
            .arg("Content-Type: application/json")
            .arg("--data")
            .arg(r#"{"jsonrpc":"2.0","method":"net_version","params":[],"id":1}"#)
            .arg("http://127.0.0.1:8545")
            .output()
        {
            if response.status.success() && !response.stdout.is_empty() {
                // Try to parse as JSON and check if it's a valid response
                if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&response.stdout) {
                    return json.get("result").is_some();
                }
            }
        }

        false
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

    pub fn start(&mut self, data_dir: &str, miner_address: Option<&str>) -> Result<(), String> {
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

        // Force kill any remaining geth processes
        #[cfg(unix)]
        {
            // Kill by name pattern
            let _ = Command::new("pkill")
                .arg("-9") // Force kill
                .arg("-f")
                .arg("geth.*--datadir.*geth-data")
                .output();

            // Also try to kill by port usage (macOS compatible)
            let _ = Command::new("sh")
                .arg("-c")
                .arg("lsof -ti:8545,30303 | xargs kill -9 2>/dev/null || true")
                .output();

            // Give it a moment to clean up
            std::thread::sleep(std::time::Duration::from_secs(1));
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

        let genesis_path = project_dir.join("genesis.json");

        // Resolve data directory relative to the executable dir if it's relative
        let data_path = self.resolve_data_dir(data_dir)?;
        if !data_path.join("geth").exists() {
            // Initialize with genesis
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
        }

        // Bootstrap node
        let bootstrap_enode = "enode://ae987db6399b50addb75d7822bfad9b4092fbfd79cbfe97e6864b1f17d3e8fcd8e9e190ad109572c1439230fa688a9837e58f0b1ad7c0dc2bc6e4ab328f3991e@130.245.173.105:30303";

        let mut cmd = Command::new(&geth_path);
        cmd.arg("--datadir")
            .arg(&data_path)
            .arg("--networkid")
            .arg("98765")
            .arg("--bootnodes")
            .arg(bootstrap_enode)
            .arg("--http")
            .arg("--http.addr")
            .arg("127.0.0.1")
            .arg("--http.port")
            .arg("8545")
            .arg("--http.api")
            .arg("eth,net,web3,personal,debug,miner,admin")
            .arg("--http.corsdomain")
            .arg("*")
            .arg("--syncmode")
            .arg("full")
            .arg("--maxpeers")
            .arg("50")
            // P2P discovery settings
            .arg("--port")
            .arg("30303") // P2P listening port
            // Network address configuration
            .arg("--nat")
            .arg("any");

        // Add this line to set a shorter IPC path
        cmd.arg("--ipcpath").arg("/tmp/chiral-geth.ipc");

        // Add miner address if provided
        if let Some(address) = miner_address {
            // Set the etherbase (coinbase) for mining rewards
            cmd.arg("--miner.etherbase").arg(address);
        }

        // Create log file for geth output
        let log_path = data_path.join("geth.log");
        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|e| format!("Failed to create log file: {}", e))?;

        cmd.stdout(Stdio::from(log_file.try_clone().unwrap()))
            .stderr(Stdio::from(log_file));

        let child = cmd
            .spawn()
            .map_err(|e| format!("Failed to start geth: {}", e))?;

        self.child = Some(child);
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
            // Kill by process name
            let result = Command::new("pkill")
                .arg("-9")
                .arg("-f")
                .arg("geth.*--datadir.*geth-data")
                .output();

            match result {
                Ok(output) => {
                    // pkill completed
                }
                Err(e) => {
                    // Failed to run pkill
                }
            }

            // Also kill by port usage
            let _ = Command::new("sh")
                .arg("-c")
                .arg("lsof -ti:8545,30303 | xargs kill -9 2>/dev/null || true")
                .output();

            std::thread::sleep(std::time::Duration::from_millis(500));
        }

        Ok(())
    }
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
    let client = reqwest::Client::new();

    let payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_getBalance",
        "params": [address, "latest"],
        "id": 1
    });

    let response = client
        .post("http://127.0.0.1:8545")
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

    Ok(format!("{:.6}", balance_ether))
}

pub async fn get_peer_count() -> Result<u32, String> {
    let client = reqwest::Client::new();

    let payload = json!({
        "jsonrpc": "2.0",
        "method": "net_peerCount",
        "params": [],
        "id": 1
    });

    let response = client
        .post("http://127.0.0.1:8545")
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

    // Convert hex to decimal
    let peer_count = u32::from_str_radix(&peer_count_hex[2..], 16)
        .map_err(|e| format!("Failed to parse peer count: {}", e))?;

    Ok(peer_count)
}

pub async fn start_mining(miner_address: &str, threads: u32) -> Result<(), String> {
    let client = reqwest::Client::new();

    // First, ensure geth is ready to accept RPC calls
    let mut attempts = 0;
    let max_attempts = 10; // 10 seconds max wait
    loop {
        // Check if geth is responding to RPC calls
        if let Ok(response) = client
            .post("http://127.0.0.1:8545")
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

    // First try to set the etherbase using miner_setEtherbase
    let set_etherbase = json!({
        "jsonrpc": "2.0",
        "method": "miner_setEtherbase",
        "params": [miner_address],
        "id": 1
    });

    let response = client
        .post("http://127.0.0.1:8545")
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

    let response = client
        .post("http://127.0.0.1:8545")
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

    Ok(())
}

pub async fn stop_mining() -> Result<(), String> {
    let client = reqwest::Client::new();

    let payload = json!({
        "jsonrpc": "2.0",
        "method": "miner_stop",
        "params": [],
        "id": 1
    });

    let response = client
        .post("http://127.0.0.1:8545")
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
    let client = reqwest::Client::new();

    let payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_mining",
        "params": [],
        "id": 1
    });

    let response = client
        .post("http://127.0.0.1:8545")
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

pub async fn get_hashrate() -> Result<String, String> {
    let client = reqwest::Client::new();

    // First try eth_hashrate
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_hashrate",
        "params": [],
        "id": 1
    });

    let response = client
        .post("http://127.0.0.1:8545")
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

        if let Ok(miner_response) = client
            .post("http://127.0.0.1:8545")
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

        // If both fail, return original error
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
    let client = reqwest::Client::new();

    let payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_blockNumber",
        "params": [],
        "id": 1
    });

    let response = client
        .post("http://127.0.0.1:8545")
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
    let client = reqwest::Client::new();

    // Get the latest block to extract difficulty
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_getBlockByNumber",
        "params": ["latest", false],
        "id": 1
    });

    let response = client
        .post("http://127.0.0.1:8545")
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

pub fn get_mining_performance(data_dir: &str) -> Result<(u64, f64), String> {
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
        // Return 0 blocks but a reasonable hashrate estimate based on CPU mining
        // Frontend will calculate blocks from actual balance
        return Ok((0, 85000.0)); // Default ~85KH/s for single thread CPU mining
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

        // Look for mining stats in logs
        // Geth may log lines like "Generating DAG" or mining-related performance
        if line.contains("Mining") && line.contains("hashrate") {
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
    }

    // If we found explicit hashrates in logs, use the average
    if !recent_hashrates.is_empty() {
        let avg_hashrate = recent_hashrates.iter().sum::<f64>() / recent_hashrates.len() as f64;
        return Ok((blocks_mined, avg_hashrate));
    }

    // Otherwise, estimate based on blocks found and difficulty
    // Look for the most recent block difficulty
    let mut last_difficulty = 0u64;
    for line in &lines {
        if line.contains("Successfully sealed new block") && line.contains("diff=") {
            if let Some(diff_pos) = line.find("diff=") {
                let diff_str = &line[diff_pos + 5..];
                if let Some(end_pos) = diff_str.find(|c: char| c == ' ' || c == '\n') {
                    let diff_val_str = &diff_str[..end_pos];
                    if let Ok(diff) = diff_val_str.parse::<u64>() {
                        last_difficulty = diff;
                        break; // Use the most recent
                    }
                }
            }
        }
    }

    // If we have blocks mined and difficulty, estimate hashrate
    // Hash rate ≈ (blocks_found * difficulty) / time_period
    // Since we're looking at recent logs, assume last ~10 minutes
    if blocks_mined > 0 && last_difficulty > 0 {
        let time_window = 600.0; // 10 minutes in seconds
        let estimated_hashrate = (blocks_mined as f64 * last_difficulty as f64) / time_window;
        return Ok((blocks_mined, estimated_hashrate));
    }

    // If still no data, check for CPU miner initialization
    for line in &lines {
        if line.contains("Updated mining threads") || line.contains("Starting mining operation") {
            // Look for thread count
            if let Some(threads_pos) = line.find("threads=") {
                let threads_str = &line[threads_pos + 8..];
                if let Some(end_pos) = threads_str.find(|c: char| c == ' ' || c == '\n') {
                    let thread_count_str = &threads_str[..end_pos];
                    if let Ok(threads) = thread_count_str.parse::<u32>() {
                        // Estimate ~85KH/s per thread for CPU mining
                        let estimated_hashrate = threads as f64 * 85000.0;
                        return Ok((blocks_mined, estimated_hashrate));
                    }
                }
            }
        }
    }

    Ok((blocks_mined, 0.0))
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

pub async fn get_mined_blocks_count(miner_address: &str) -> Result<u64, String> {
    let client = reqwest::Client::new();
    let mut blocks_mined = 0u64;

    // Get the current block number
    let block_number_payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_blockNumber",
        "params": [],
        "id": 1
    });

    let response = client
        .post("http://127.0.0.1:8545")
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

    // Check recent blocks (last 100 or current block count, whichever is smaller)
    let blocks_to_check = std::cmp::min(1000, current_block);
    let start_block = current_block.saturating_sub(blocks_to_check).max(1);

    // Normalize the miner address for comparison
    let normalized_miner = miner_address.to_lowercase();

    for block_num in start_block..=current_block {
        let block_payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_getBlockByNumber",
            "params": [format!("0x{:x}", block_num), false],
            "id": 1
        });

        if let Ok(response) = client
            .post("http://127.0.0.1:8545")
            .json(&block_payload)
            .send()
            .await
        {
            if let Ok(json_response) = response.json::<serde_json::Value>().await {
                if let Some(block) = json_response.get("result") {
                    if let Some(miner) = block.get("miner").and_then(|m| m.as_str()) {
                        if miner.to_lowercase() == normalized_miner {
                            blocks_mined += 1;
                        }
                    }
                }
            }
        }
    }

    Ok(blocks_mined)
}

//Fetching Recent Blocks Mined by address, scanning backwards from latest
pub async fn get_recent_mined_blocks(
    miner_address: &str,
    lookback: u64,
    limit: usize,
) -> Result<Vec<MinedBlock>, String> {
    let client = reqwest::Client::new();

    // Fetch latest block number
    let latest_v = client
        .post("http://127.0.0.1:8545")
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

        let block_v = client
            .post("http://127.0.0.1:8545")
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

        // Since Geth's default reward (2.0) doesn't match the intended Chiral Network
        // reward, we hardcode the intended value of 5.0 here.
        let reward = Some(2.0);

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

pub async fn get_network_hashrate() -> Result<String, String> {
    let client = reqwest::Client::new();

    // First, try to get the actual network hashrate from eth_hashrate
    // This will return the sum of all miners that have submitted their hashrate
    let hashrate_payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_hashrate",
        "params": [],
        "id": 1
    });

    if let Ok(response) = client
        .post("http://127.0.0.1:8545")
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

    let response = client
        .post("http://127.0.0.1:8545")
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

    // For Chiral private network, estimate network hashrate from difficulty
    // The relationship between hashrate and difficulty depends on the algorithm
    // For Ethash on a private network with CPU mining:
    // Network Hashrate ≈ Difficulty / Block Time
    // This gives us the hash rate needed to mine a block at this difficulty
    let estimated_block_time = 15.0; // seconds (Ethereum default)
    let hashrate = difficulty as f64 / estimated_block_time;

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

    let provider = Provider::<Http>::try_from("http://127.0.0.1:8545")
        .map_err(|e| format!("Failed to connect to Geth: {}", e))?;

    let chain_id = 98765u64;
    let wallet = wallet.with_chain_id(chain_id);

    let client = SignerMiddleware::new(provider.clone(), wallet);

    let to: Address = to_address
        .parse()
        .map_err(|e| format!("Invalid to address: {}", e))?;

    let amount_wei = U256::from((amount_chiral * 1_000_000_000_000_000_000.0) as u128);

    // Get nonce for pending block (includes pending transactions)
    let from_addr: Address = from_address
        .parse()
        .map_err(|e| format!("Invalid from address: {}", e))?;

    let nonce = provider
        .get_transaction_count(from_addr, Some(BlockNumber::Pending.into()))
        .await
        .map_err(|e| format!("Failed to get nonce: {}", e))?;

    let gas_price = provider
        .get_gas_price()
        .await
        .map_err(|e| format!("Failed to get gas price: {}", e))?;

    // Increase gas price by 10% to ensure it's not underpriced
    let gas_price_adjusted = gas_price * 110 / 100;

    let tx = TransactionRequest::new()
        .to(to)
        .value(amount_wei)
        .gas(21000)
        .gas_price(gas_price_adjusted)
        .nonce(nonce);

    let pending_tx = client
        .send_transaction(tx, None)
        .await
        .map_err(|e| format!("Failed to send transaction: {}", e))?;

    let tx_hash = format!("{:?}", pending_tx.tx_hash());

    Ok(tx_hash)
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
        .post("http://127.0.0.1:8545")
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
