use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{Manager, State};
use totp_rs::{Algorithm, Secret, TOTP};
use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;
use rand::RngCore;
use serde_json;

// This struct will hold the address of the currently logged-in account.
// It needs to be added to Tauri's state management in `main.rs`.
pub struct ActiveAccount(pub Mutex<Option<String>>);

#[derive(serde::Serialize)]
pub struct TotpSetupInfo {
    secret: String,
    otpauth_url: String,
}

// Helper to get the file path for a 2FA secret.
// It uses a hash of the address for the filename to avoid issues with special characters.
fn get_2fa_file_path(app_handle: &tauri::AppHandle, address: &str) -> Result<PathBuf, String> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .ok_or_else(|| "Could not get app data directory".to_string())?;

    let two_fa_dir = app_data_dir.join("2fa_secrets");
    fs::create_dir_all(&two_fa_dir).map_err(|e| format!("Failed to create 2FA directory: {}", e))?;

    let filename = format!("{:x}", sha256::digest(address.as_bytes()));
    Ok(two_fa_dir.join(filename))
}

// Helper to get the address of the currently active account.
// It handles locking the mutex and checking for a valid session.
fn get_active_address(active_account: &State<'_, ActiveAccount>) -> Result<String, String> {
    const POISONED_MUTEX_ERROR: &str = "Failed to acquire lock on active account state.";
    let address_lock = active_account.0.lock().map_err(|_| POISONED_MUTEX_ERROR.to_string())?;
    address_lock
        .as_deref()
        .map(String::from)
        .ok_or_else(|| "No account is currently active. Please log in.".to_string())
}

/// Generate a random salt for encryption
fn generate_random_salt() -> [u8; 32] {
    let mut salt = [0u8; 32];
    OsRng.fill_bytes(&mut salt);
    salt
}

/// Derive encryption key from password and salt
fn derive_encryption_key(password: &str, salt: &[u8]) -> [u8; 32] {
    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, 100_000, &mut key);
    key
}

/// Encrypt 2FA secret using AES-256-GCM
fn encrypt_2fa_secret(secret: &str, password: &str, salt: &[u8]) -> Result<String, String> {
    let key = derive_encryption_key(password, salt);
    let key = Key::<Aes256Gcm>::from_slice(&key);
    let cipher = Aes256Gcm::new(key);

    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, secret.as_bytes())
        .map_err(|e| format!("Encryption failed: {}", e))?;

    let combined = [nonce.as_slice(), &ciphertext].concat();
    Ok(hex::encode(combined))
}

/// Decrypt 2FA secret using AES-256-GCM
fn decrypt_2fa_secret(encrypted_hex: &str, password: &str, salt: &[u8]) -> Result<String, String> {
    let combined = hex::decode(encrypted_hex).map_err(|e| format!("Invalid hex: {}", e))?;

    if combined.len() < 12 {
        return Err("Invalid encrypted data length".to_string());
    }

    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);

    let key = derive_encryption_key(password, salt);
    let key = Key::<Aes256Gcm>::from_slice(&key);
    let cipher = Aes256Gcm::new(key);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("Decryption failed: {}", e))?;

    String::from_utf8(plaintext).map_err(|e| format!("Invalid UTF-8: {}", e))
}

/// Checks if 2FA is enabled for the currently active account.
#[tauri::command]
pub fn is_2fa_enabled(
    app_handle: tauri::AppHandle,
    active_account: State<'_, ActiveAccount>,
) -> Result<bool, String> {
    let address = get_active_address(&active_account)?;
    let path = get_2fa_file_path(&app_handle, &address)?;
    Ok(path.exists())
}

/// Generates a new TOTP secret and an otpauth:// URL for QR code generation.
#[tauri::command]
pub fn generate_totp_secret(active_account: State<'_, ActiveAccount>) -> Result<TotpSetupInfo, String> {
    let address = get_active_address(&active_account)?;

    let secret = Secret::default();

    let totp = TOTP::new(
        Algorithm::SHA256, // Use a stronger algorithm than SHA1
        6,  // 6 digits
        1,  // 1 step of skew is allowed
        30, // 30-second step size
        secret.to_bytes().map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    let issuer = "Chiral Network";
    let account_name = &address;

    let otpauth_url = totp.get_url(account_name, issuer);

    Ok(TotpSetupInfo {
        secret: secret.to_b32(), // Base32 representation of the secret
        otpauth_url,
    })
}

/// Verifies the initial TOTP code and saves the secret if it's valid.
#[tauri::command]
pub fn verify_and_enable_totp(
    secret: String,
    code: String,
    password: String,
    app_handle: tauri::AppHandle,
    active_account: State<'_, ActiveAccount>,
) -> Result<bool, String> {
    let address = get_active_address(&active_account)?;

    let secret_bytes = Secret::from_b32(&secret)
        .map_err(|e| format!("Invalid secret format: {}", e))?
        .to_bytes()
        .map_err(|e| e.to_string())?;

    let totp = TOTP::new(Algorithm::SHA256, 6, 1, 30, secret_bytes).map_err(|e| e.to_string())?;

    if totp.check_current(&code).unwrap_or(false) {
        let path = get_2fa_file_path(&app_handle, &address)?;
        // SECURITY: Encrypt the 2FA secret using AES-256-GCM with password-derived key
        let salt = generate_random_salt();
        let encrypted_secret = encrypt_2fa_secret(&secret, password, &salt)?;
        let data_to_store = serde_json::json!({
            "encrypted_secret": encrypted_secret,
            "salt": hex::encode(&salt)
        });
        fs::write(&path, data_to_store.to_string()).map_err(|e| format!("Failed to save 2FA secret: {}", e))?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Verifies a TOTP code against the stored secret for a sensitive action.
#[tauri::command]
pub fn verify_totp_code(
    code: String,
    password: String,
    app_handle: tauri::AppHandle,
    active_account: State<'_, ActiveAccount>,
) -> Result<bool, String> {
    let address = get_active_address(&active_account)?;

    let path = get_2fa_file_path(&app_handle, &address)?;
    if !path.exists() {
        return Err("2FA is not enabled for this account.".to_string());
    }

    let stored_data = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read stored 2FA secret: {}", e))?;

    let json_data: serde_json::Value = serde_json::from_str(&stored_data)
        .map_err(|e| format!("Failed to parse 2FA data: {}", e))?;

    let encrypted_secret = json_data["encrypted_secret"].as_str()
        .ok_or("Missing encrypted_secret field")?;
    let salt_hex = json_data["salt"].as_str()
        .ok_or("Missing salt field")?;

    let salt = hex::decode(salt_hex).map_err(|e| format!("Invalid salt: {}", e))?;
    let secret_b32 = decrypt_2fa_secret(encrypted_secret, &password, &salt)?;

    let secret_bytes = Secret::from_b32(&secret_b32)
        .map_err(|e| format!("Invalid stored secret: {}", e))?
        .to_bytes()
        .map_err(|e| e.to_string())?;

    let totp = TOTP::new(Algorithm::SHA256, 6, 1, 30, secret_bytes).map_err(|e| e.to_string())?;

    Ok(totp.check_current(&code).unwrap_or(false))
}

/// Disables 2FA by deleting the stored secret.
#[tauri::command]
pub fn disable_2fa(app_handle: tauri::AppHandle, active_account: State<'_, ActiveAccount>) -> Result<(), String> {
    let address = get_active_address(&active_account)?;

    let path = get_2fa_file_path(&app_handle, &address)?;
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("Failed to remove 2FA secret: {}", e))?;
    }
    Ok(())
}

/// Sets the active account in the backend state (e.g., after a successful login).
#[tauri::command]
pub fn login(address: String, active_account: State<'_, ActiveAccount>) {
    let mut address_lock = active_account.0.lock().unwrap();
    *address_lock = Some(address);
}