use rand::rngs::OsRng;
use secp256k1::{PublicKey, Secp256k1, SecretKey};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha3::{Digest, Keccak256};
use std::process::{Child, Command};
use std::path::PathBuf;

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
        self.child.is_some()
    }

    pub fn start(&mut self, data_dir: &str) -> Result<(), String> {
        if self.child.is_some() {
            return Err("Geth is already running".to_string());
        }

        // Use the project directory as base (works for both dev and production)
        let project_dir = if cfg!(debug_assertions) {
            // In development, use the workspace directory
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .ok_or("Failed to get project dir")?
                .to_path_buf()
        } else {
            // In production, use relative to the executable
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

        let geth_path = project_dir.join("src-tauri").join("bin").join("geth");
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

        let child = Command::new(&geth_path)
            .arg("--datadir")
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
            .arg("eth,net,web3,personal,debug")
            .arg("--http.corsdomain")
            .arg("*")
            .arg("--syncmode")
            .arg("full")
            .arg("--maxpeers")
            .arg("50")
            .spawn()
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