use aes::cipher::{KeyIvInit, StreamCipher};
use aes::Aes256;
use ctr::Ctr128BE;
use directories::ProjectDirs;
use hmac::Hmac;
use pbkdf2::pbkdf2;
use rand::{thread_rng, RngCore};
use serde::{Deserialize, Serialize};
use sha3::Sha3_256;
use std::fs;
use std::path::PathBuf;

type Aes256Ctr = Ctr128BE<Aes256>;

#[derive(Debug, Serialize, Deserialize)]
pub struct EncryptedKeystore {
    pub address: String,
    pub encrypted_private_key: String,
    pub salt: String,
    pub iv: String,
    // The 2FA secret, encrypted with the same key as the private key, but with its own IV.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encrypted_two_fa_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub two_fa_iv: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Keystore {
    pub accounts: Vec<EncryptedKeystore>,
}

impl Keystore {
    pub fn new() -> Self {
        Keystore {
            accounts: Vec::new(),
        }
    }

    pub fn get_keystore_path() -> Result<PathBuf, String> {
        let proj_dirs = ProjectDirs::from("com", "chiral", "network")
            .ok_or_else(|| "Could not determine project directories".to_string())?;

        let data_dir = proj_dirs.data_dir();

        // Create directory if it doesn't exist
        fs::create_dir_all(data_dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        Ok(data_dir.join("keystore.json"))
    }

    pub fn load() -> Result<Self, String> {
        let path = Self::get_keystore_path()?;

        if !path.exists() {
            return Ok(Self::new());
        }

        let contents =
            fs::read_to_string(&path).map_err(|e| format!("Failed to read keystore: {}", e))?;

        serde_json::from_str(&contents).map_err(|e| format!("Failed to parse keystore: {}", e))
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::get_keystore_path()?;

        let contents = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize keystore: {}", e))?;

        fs::write(&path, contents).map_err(|e| format!("Failed to write keystore: {}", e))?;

        Ok(())
    }

    pub fn add_account(
        &mut self,
        address: String,
        private_key: &str,
        password: &str,
    ) -> Result<(), String> {
        let (encrypted, salt, iv) = encrypt_private_key(private_key, password)?;

        // Remove existing account with same address
        self.accounts.retain(|a| a.address != address);

        self.accounts.push(EncryptedKeystore {
            address,
            encrypted_private_key: encrypted,
            salt,
            iv,
            encrypted_two_fa_secret: None,
            two_fa_iv: None,
        });

        self.save()?;
        Ok(())
    }

    pub fn get_account(&self, address: &str, password: &str) -> Result<String, String> {
        let account = self
            .accounts
            .iter()
            .find(|a| a.address == address)
            .ok_or_else(|| "Account not found".to_string())?;

        decrypt_private_key(
            &account.encrypted_private_key,
            &account.salt,
            &account.iv,
            password,
        )
    }

    pub fn is_2fa_enabled(&self, address: &str) -> Result<bool, String> {
        let account = self
            .accounts
            .iter()
            .find(|a| a.address == address)
            .ok_or_else(|| "Account not found".to_string())?;
        Ok(account.encrypted_two_fa_secret.is_some())
    }

    pub fn get_2fa_secret(&self, address: &str, password: &str) -> Result<Option<String>, String> {
        let account = self
            .accounts
            .iter()
            .find(|a| a.address == address)
            .ok_or_else(|| "Account not found".to_string())?;

        match (&account.encrypted_two_fa_secret, &account.two_fa_iv) {
            (Some(encrypted_secret), Some(iv)) => {
                let decrypted_secret = decrypt_data(encrypted_secret, &account.salt, iv, password)?;
                Ok(Some(decrypted_secret))
            }
            _ => Ok(None),
        }
    }

    pub fn set_2fa_secret(
        &mut self,
        address: &str,
        secret: &str,
        password: &str,
    ) -> Result<(), String> {
        let account = self
            .accounts
            .iter_mut()
            .find(|a| a.address == address)
            .ok_or_else(|| "Account not found".to_string())?;

        let (encrypted_secret, iv) = encrypt_data(secret, password, &account.salt)?;
        account.encrypted_two_fa_secret = Some(encrypted_secret);
        account.two_fa_iv = Some(iv);

        self.save()
    }

    pub fn remove_2fa_secret(&mut self, address: &str, password: &str) -> Result<(), String> {
        let account = self
            .accounts
            .iter_mut()
            .find(|a| a.address == address)
            .ok_or_else(|| "Account not found".to_string())?;

        // To remove, we must first verify the password is correct.
        // We can do this by trying to decrypt the existing secret.
        if let (Some(encrypted_secret), Some(iv)) =
            (&account.encrypted_two_fa_secret, &account.two_fa_iv)
        {
            decrypt_data(encrypted_secret, &account.salt, iv, password)
                .map_err(|_| "Invalid password. Cannot disable 2FA.".to_string())?;
        }

        // Password is correct, so we can remove the secret.
        account.encrypted_two_fa_secret = None;
        account.two_fa_iv = None;
        self.save()
    }

    pub fn remove_account(&mut self, address: &str) -> Result<(), String> {
        self.accounts.retain(|a| a.address != address);
        self.save()?;
        Ok(())
    }

    pub fn list_accounts(&self) -> Vec<String> {
        self.accounts.iter().map(|a| a.address.clone()).collect()
    }
}

fn derive_key(password: &str, salt: &[u8]) -> [u8; 32] {
    let mut key = [0u8; 32];
    pbkdf2::<Hmac<Sha3_256>>(password.as_bytes(), salt, 4096, &mut key)
        .expect("PBKDF2 should not fail");
    key
}

fn encrypt_private_key(
    private_key: &str,
    password: &str,
) -> Result<(String, String, String), String> {
    let mut rng = thread_rng();

    // Generate random salt
    let mut salt = [0u8; 32];
    rng.fill_bytes(&mut salt);

    // Generate random IV
    let mut iv = [0u8; 16];
    rng.fill_bytes(&mut iv);

    // Derive key from password
    let key = derive_key(password, &salt);

    // Encrypt
    let mut data = private_key.as_bytes().to_vec();
    let mut cipher = Aes256Ctr::new(&key.into(), &iv.into());
    cipher.apply_keystream(&mut data);

    Ok((hex::encode(data), hex::encode(salt), hex::encode(iv)))
}

/// Generic function to encrypt any string data using the password and a salt.
fn encrypt_data(
    data_to_encrypt: &str,
    password: &str,
    salt_hex: &str,
) -> Result<(String, String), String> {
    let salt = hex::decode(salt_hex).map_err(|e| format!("Invalid salt: {}", e))?;
    let key = derive_key(password, &salt);

    let mut iv = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut iv);

    let mut data = data_to_encrypt.as_bytes().to_vec();
    let mut cipher = Aes256Ctr::new(&key.into(), &iv.into());
    cipher.apply_keystream(&mut data);

    Ok((hex::encode(data), hex::encode(iv)))
}

/// Generic function to decrypt data.
fn decrypt_data(
    encrypted_hex: &str,
    salt_hex: &str,
    iv_hex: &str,
    password: &str,
) -> Result<String, String> {
    decrypt_private_key(encrypted_hex, salt_hex, iv_hex, password)
}

fn decrypt_private_key(
    encrypted: &str,
    salt: &str,
    iv: &str,
    password: &str,
) -> Result<String, String> {
    // Decode hex
    let salt_bytes = hex::decode(salt).map_err(|e| format!("Invalid salt: {}", e))?;
    let iv_bytes = hex::decode(iv).map_err(|e| format!("Invalid IV: {}", e))?;
    let mut ciphertext =
        hex::decode(encrypted).map_err(|e| format!("Invalid ciphertext: {}", e))?;

    // Derive key from password
    let key = derive_key(password, &salt_bytes);

    // Decrypt
    let iv_array: [u8; 16] = iv_bytes
        .try_into()
        .map_err(|_| "Invalid IV length".to_string())?;

    let mut cipher = Aes256Ctr::new(&key.into(), &iv_array.into());
    cipher.apply_keystream(&mut ciphertext);

    String::from_utf8(ciphertext)
        .map_err(|_| "Decryption failed: incorrect password or corrupted data".to_string())
}
