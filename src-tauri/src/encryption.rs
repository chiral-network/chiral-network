use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
// PBKDF2 imports handled in function
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use tokio::fs;

/// Encryption configuration and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionInfo {
    pub method: String,
    pub key_fingerprint: String,
    pub nonce: Vec<u8>,
    pub salt: Vec<u8>,
}

/// Result of file encryption operation
#[derive(Debug)]
pub struct EncryptionResult {
    pub encrypted_file_path: String,
    pub encryption_info: EncryptionInfo,
    pub original_size: u64,
    pub encrypted_size: u64,
}

/// File encryption service
pub struct FileEncryption;

impl FileEncryption {
    /// Generate a secure encryption key from password using PBKDF2
    pub fn derive_key_from_password(password: &str, salt: &[u8]) -> Result<[u8; 32], String> {
        use pbkdf2::pbkdf2_hmac;
        use sha2::Sha256;

        let password_bytes = password.as_bytes();
        let mut key = [0u8; 32];

        pbkdf2_hmac::<Sha256>(password_bytes, salt, 100_000, &mut key);

        Ok(key)
    }

    /// Generate a random encryption key
    pub fn generate_random_key() -> [u8; 32] {
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        key
    }

    /// Generate key fingerprint for identification
    pub fn generate_key_fingerprint(key: &[u8; 32]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(key);
        let hash = hasher.finalize();
        hex::encode(&hash[..8]) // Use first 8 bytes as fingerprint
    }

    /// Encrypt a file using AES-256-GCM
    pub async fn encrypt_file(
        input_path: &Path,
        output_path: &Path,
        key: &[u8; 32],
    ) -> Result<EncryptionResult, String> {
        // Read the input file
        let plaintext = fs::read(input_path)
            .await
            .map_err(|e| format!("Failed to read input file: {}", e))?;

        let original_size = plaintext.len() as u64;

        // Create cipher
        let key = Key::<Aes256Gcm>::from_slice(key);
        let cipher = Aes256Gcm::new(key);

        // Generate random nonce
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

        // Encrypt the file
        let ciphertext = cipher
            .encrypt(&nonce, plaintext.as_ref())
            .map_err(|e| format!("Encryption failed: {}", e))?;

        // Write encrypted file
        fs::write(output_path, &ciphertext)
            .await
            .map_err(|e| format!("Failed to write encrypted file: {}", e))?;

        let encrypted_size = ciphertext.len() as u64;

        // Generate salt for key derivation (even if using random key)
        let mut salt = [0u8; 16];
        OsRng.fill_bytes(&mut salt);

        let encryption_info = EncryptionInfo {
            method: "AES-256-GCM".to_string(),
            key_fingerprint: Self::generate_key_fingerprint(key.as_slice().try_into().unwrap()),
            nonce: nonce.to_vec(),
            salt: salt.to_vec(),
        };

        Ok(EncryptionResult {
            encrypted_file_path: output_path.to_string_lossy().to_string(),
            encryption_info,
            original_size,
            encrypted_size,
        })
    }

    /// Decrypt a file using AES-256-GCM
    pub async fn decrypt_file(
        input_path: &Path,
        output_path: &Path,
        key: &[u8; 32],
        encryption_info: &EncryptionInfo,
    ) -> Result<u64, String> {
        // Verify encryption method
        if encryption_info.method != "AES-256-GCM" {
            return Err(format!(
                "Unsupported encryption method: {}",
                encryption_info.method
            ));
        }

        // Verify key fingerprint
        let expected_fingerprint = Self::generate_key_fingerprint(key);
        if encryption_info.key_fingerprint != expected_fingerprint {
            return Err("Invalid decryption key (fingerprint mismatch)".to_string());
        }

        // Read encrypted file
        let ciphertext = fs::read(input_path)
            .await
            .map_err(|e| format!("Failed to read encrypted file: {}", e))?;

        // Create cipher
        let key = Key::<Aes256Gcm>::from_slice(key);
        let cipher = Aes256Gcm::new(key);

        // Extract nonce
        if encryption_info.nonce.len() != 12 {
            return Err("Invalid nonce length".to_string());
        }
        let nonce = Nonce::from_slice(&encryption_info.nonce);

        // Decrypt the file
        let plaintext = cipher
            .decrypt(nonce, ciphertext.as_ref())
            .map_err(|e| format!("Decryption failed: {}", e))?;

        // Write decrypted file
        fs::write(output_path, &plaintext)
            .await
            .map_err(|e| format!("Failed to write decrypted file: {}", e))?;

        Ok(plaintext.len() as u64)
    }

    /// Encrypt file with password
    pub async fn encrypt_file_with_password(
        input_path: &Path,
        output_path: &Path,
        password: &str,
    ) -> Result<EncryptionResult, String> {
        // Generate random salt
        let mut salt = [0u8; 16];
        OsRng.fill_bytes(&mut salt);

        // Derive key from password
        let key = Self::derive_key_from_password(password, &salt)?;

        // Encrypt file
        let mut result = Self::encrypt_file(input_path, output_path, &key).await?;

        // Update salt in encryption info
        result.encryption_info.salt = salt.to_vec();

        Ok(result)
    }

    /// Decrypt file with password
    pub async fn decrypt_file_with_password(
        input_path: &Path,
        output_path: &Path,
        password: &str,
        encryption_info: &EncryptionInfo,
    ) -> Result<u64, String> {
        // Derive key from password and salt
        let key = Self::derive_key_from_password(password, &encryption_info.salt)?;

        // Decrypt file
        Self::decrypt_file(input_path, output_path, &key, encryption_info).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::fs;

    #[tokio::test]
    async fn test_file_encryption_random_key() {
        let dir = tempdir().unwrap();
        let input_path = dir.path().join("test_input.txt");
        let output_path = dir.path().join("test_encrypted.bin");
        let decrypted_path = dir.path().join("test_decrypted.txt");

        // Create test file
        let test_content = "Hello, this is a test file for encryption!";
        fs::write(&input_path, test_content).await.unwrap();

        // Generate key and encrypt
        let key = FileEncryption::generate_random_key();
        let result = FileEncryption::encrypt_file(&input_path, &output_path, &key)
            .await
            .unwrap();

        assert_eq!(result.original_size, test_content.len() as u64);
        assert!(result.encrypted_size > 0);

        // Decrypt file
        let decrypted_size = FileEncryption::decrypt_file(
            &output_path,
            &decrypted_path,
            &key,
            &result.encryption_info,
        )
        .await
        .unwrap();

        // Verify decrypted content
        let decrypted_content = fs::read_to_string(&decrypted_path).await.unwrap();
        assert_eq!(decrypted_content, test_content);
        assert_eq!(decrypted_size, test_content.len() as u64);
    }

    #[tokio::test]
    async fn test_file_encryption_with_password() {
        let dir = tempdir().unwrap();
        let input_path = dir.path().join("test_input.txt");
        let output_path = dir.path().join("test_encrypted.bin");
        let decrypted_path = dir.path().join("test_decrypted.txt");

        // Create test file
        let test_content = "Hello, this is a test file for password encryption!";
        fs::write(&input_path, test_content).await.unwrap();

        let password = "super_secure_password_123";

        // Encrypt with password
        let result =
            FileEncryption::encrypt_file_with_password(&input_path, &output_path, password)
                .await
                .unwrap();

        // Decrypt with password
        let decrypted_size = FileEncryption::decrypt_file_with_password(
            &output_path,
            &decrypted_path,
            password,
            &result.encryption_info,
        )
        .await
        .unwrap();

        // Verify decrypted content
        let decrypted_content = fs::read_to_string(&decrypted_path).await.unwrap();
        assert_eq!(decrypted_content, test_content);
        assert_eq!(decrypted_size, test_content.len() as u64);
    }

    #[tokio::test]
    async fn test_wrong_password_fails() {
        let dir = tempdir().unwrap();
        let input_path = dir.path().join("test_input.txt");
        let output_path = dir.path().join("test_encrypted.bin");
        let decrypted_path = dir.path().join("test_decrypted.txt");

        // Create test file
        fs::write(&input_path, "test content").await.unwrap();

        let correct_password = "correct_password";
        let wrong_password = "wrong_password";

        // Encrypt with correct password
        let result =
            FileEncryption::encrypt_file_with_password(&input_path, &output_path, correct_password)
                .await
                .unwrap();

        // Try to decrypt with wrong password - should fail
        let decrypt_result = FileEncryption::decrypt_file_with_password(
            &output_path,
            &decrypted_path,
            wrong_password,
            &result.encryption_info,
        )
        .await;

        assert!(decrypt_result.is_err());
        assert!(decrypt_result.unwrap_err().contains("fingerprint mismatch"));
    }
}
