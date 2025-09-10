use rand::rngs::OsRng;
use secp256k1::{PublicKey, Secp256k1, SecretKey};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha3::{Digest, Keccak256};
use std::process::{Child, Command, Stdio};
use std::path::PathBuf;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufRead};

#[derive(Debug, Serialize, Deserialize)]
pub struct EthAccount {
    pub address: String,
    pub private_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountInfo {
    pub address: String,
    pub balance: String,
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

    pub fn start(&mut self, data_dir: &str, miner_address: Option<&str>) -> Result<(), String> {
        // Check if we already have a tracked child process
        if self.child.is_some() {
            return Ok(()); // Already running, no need to start again
        }
        
        // Check if geth is already running on the system (from a previous session)
        if self.is_running() {
            println!("Geth is already running from a previous session");
            return Ok(()); // Already running externally
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

        // Check if datadir needs initialization
        let data_path = PathBuf::from(data_dir);
        if !data_path.join("geth").exists() {
            // Initialize with genesis
            let init_output = Command::new(&geth_path)
                .arg("--datadir")
                .arg(data_dir)
                .arg("init")
                .arg(&genesis_path)
                .output()
                .map_err(|e| format!("Failed to initialize genesis: {}", e))?;

            if !init_output.status.success() {
                return Err(format!("Failed to init genesis: {}", 
                    String::from_utf8_lossy(&init_output.stderr)));
            }
        }

        // Bootstrap node
        let bootstrap_enode = "enode://ae987db6399b50addb75d7822bfad9b4092fbfd79cbfe97e6864b1f17d3e8fcd8e9e190ad109572c1439230fa688a9837e58f0b1ad7c0dc2bc6e4ab328f3991e@130.245.173.105:30303";

        let mut cmd = Command::new(&geth_path);
        cmd.arg("--datadir")
            .arg(data_dir)
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
            .arg("eth,net,web3,personal,debug,miner")
            .arg("--http.corsdomain")
            .arg("*")
            .arg("--syncmode")
            .arg("full")
            .arg("--maxpeers")
            .arg("50");
        
        // Add miner address if provided
        if let Some(address) = miner_address {
            println!("Setting miner.etherbase to: {}", address);
            // Set the etherbase (coinbase) for mining rewards
            cmd.arg("--miner.etherbase").arg(address);
        } else {
            println!("No miner address provided, starting without etherbase");
        }
        
        // Create log file for geth output
        let log_path = PathBuf::from(data_dir).join("geth.log");
        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|e| format!("Failed to create log file: {}", e))?;
        
        cmd.stdout(Stdio::from(log_file.try_clone().unwrap()))
            .stderr(Stdio::from(log_file));
        
        let child = cmd.spawn()
            .map_err(|e| format!("Failed to start geth: {}", e))?;

        self.child = Some(child);
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), String> {
        if let Some(mut child) = self.child.take() {
            child
                .kill()
                .map_err(|e| format!("Failed to stop geth: {}", e))?;
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
    
    let private_key_bytes = hex::decode(private_key_hex)
        .map_err(|e| format!("Invalid hex private key: {}", e))?;
    
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
        return Err(format!("RPC error: {}", error));
    }
    
    let hashrate_hex = json_response["result"]
        .as_str()
        .ok_or("Invalid hashrate response")?;
    
    // Convert hex to decimal
    let hashrate = u64::from_str_radix(&hashrate_hex[2..], 16)
        .map_err(|e| format!("Failed to parse hashrate: {}", e))?;
    
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

pub fn get_mining_logs(data_dir: &str, lines: usize) -> Result<Vec<String>, String> {
    let log_path = PathBuf::from(data_dir).join("geth.log");
    
    if !log_path.exists() {
        return Ok(vec!["No logs available yet.".to_string()]);
    }
    
    let file = File::open(&log_path)
        .map_err(|e| format!("Failed to open log file: {}", e))?;
    
    let reader = BufReader::new(file);
    let all_lines: Vec<String> = reader
        .lines()
        .filter_map(Result::ok)
        .collect();
    
    // Get the last N lines
    let start = if all_lines.len() > lines {
        all_lines.len() - lines
    } else {
        0
    };
    
    Ok(all_lines[start..].to_vec())
}

pub async fn get_network_hashrate() -> Result<String, String> {
    let client = reqwest::Client::new();
    
    // Get the latest block and previous block to calculate network hashrate
    let latest_block = json!({
        "jsonrpc": "2.0",
        "method": "eth_getBlockByNumber",
        "params": ["latest", false],
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
    
    // Estimate network hashrate (difficulty / block time)
    // Assuming 15 second block time for ETC
    let hashrate = difficulty / 15;
    
    // Convert to human-readable format
    let formatted = if hashrate >= 1_000_000_000_000 {
        format!("{:.2} TH/s", hashrate as f64 / 1_000_000_000_000.0)
    } else if hashrate >= 1_000_000_000 {
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