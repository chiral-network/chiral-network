use rand::rngs::OsRng;
use secp256k1::{PublicKey, Secp256k1, SecretKey};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use std::process::{Child, Command};
use std::sync::Mutex;

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

    pub fn start(&mut self, data_dir: &str) -> Result<(), String> {
        if self.child.is_some() {
            return Err("Geth is already running".to_string());
        }

        let child = Command::new("geth")
            .arg("--classic")
            .arg("--datadir")
            .arg(data_dir)
            .arg("--http")
            .arg("--http.addr")
            .arg("127.0.0.1")
            .arg("--http.port")
            .arg("8545")
            .arg("--http.api")
            .arg("eth,net,web3,personal")
            .arg("--http.corsdomain")
            .arg("*")
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