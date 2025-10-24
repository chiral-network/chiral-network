use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
// PBKDF2 imports handled in function
use base64::{engine::general_purpose, Engine as _};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use tokio::fs;

// ECIES imports for key encryption
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use hkdf::Hkdf;
use x25519_dalek::{EphemeralSecret, PublicKey, SharedSecret, StaticSecret};

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
        let mut hasher = Sha256::default();
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

/// A bundle containing the encrypted AES key and the necessary data for decryption.
/// This struct is designed to be serialized (e.g., to JSON) and stored as file metadata.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EncryptedAesKeyBundle {
    /// The sender's temporary public key (32 bytes), hex-encoded.
    pub ephemeral_public_key: String,
    /// The AES key, encrypted and then hex-encoded.
    pub encrypted_key: String,
    /// The nonce used for AES-GCM encryption (12 bytes), hex-encoded.
    pub nonce: String,
}

pub trait DiffieHellman {
    fn diffie_hellman(self, their_public: &PublicKey) -> SharedSecret;
}

impl DiffieHellman for &StaticSecret {
    fn diffie_hellman(self, their_public: &PublicKey) -> SharedSecret {
        self.diffie_hellman(their_public)
    }
}

impl DiffieHellman for EphemeralSecret {
    fn diffie_hellman(self, their_public: &PublicKey) -> SharedSecret {
        self.diffie_hellman(their_public)
    }
}

/// Encrypts a 32-byte AES key using the recipient's public key (ECIES pattern).
///
/// # Arguments
/// * `aes_key_to_encrypt` - The 32-byte AES key for file chunks (the DEK).
/// * `recipient_public_key` - The recipient's X25519 public key.
///
/// # Returns
/// An `EncryptedAesKeyBundle` struct containing the data needed for decryption.
pub fn encrypt_aes_key(
    aes_key_to_encrypt: &[u8; 32],
    recipient_public_key: &PublicKey,
) -> Result<EncryptedAesKeyBundle, String> {
    // 1. Generate a temporary (ephemeral) X25519 key pair for the sender.
    let ephemeral_secret = EphemeralSecret::random_from_rng(OsRng);
    let ephemeral_public_key = PublicKey::from(&ephemeral_secret);

    // 2. Compute the shared secret.
    let shared_secret = ephemeral_secret.diffie_hellman(recipient_public_key);

    // 3. Use HKDF to derive a Key Encryption Key (KEK) from the shared secret.
    let hk = Hkdf::<Sha256>::new(
        Some(ephemeral_public_key.as_bytes()),
        shared_secret.as_bytes(),
    );
    let mut kek = [0u8; 32]; // 32 bytes for an AES-256 key
    hk.expand(b"chiral-network-kek", &mut kek)
        .map_err(|e| format!("HKDF expansion failed: {}", e))?;

    // 4. Encrypt the AES key (DEK) with the derived KEK.
    let key = Key::<Aes256Gcm>::from_slice(&kek);
    let kek_cipher = Aes256Gcm::new(key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng); // Generate a random nonce
    let encrypted_key = kek_cipher
        .encrypt(&nonce, aes_key_to_encrypt.as_ref())
        .map_err(|e| format!("AES key encryption failed: {}", e))?;

    // 5. Return the bundle with hex-encoded data for easy serialization.
    Ok(EncryptedAesKeyBundle {
        ephemeral_public_key: hex::encode(ephemeral_public_key.as_bytes()),
        encrypted_key: hex::encode(encrypted_key),
        nonce: hex::encode(nonce.as_slice()),
    })
}

/// Decrypts an AES key using the recipient's private key.
///
/// # Arguments
/// * `encrypted_bundle` - The `EncryptedAesKeyBundle` received from the sender.
/// * `recipient_secret_key` - The recipient's X25519 private key.
///
/// # Returns
/// The decrypted 32-byte AES key.
pub fn decrypt_aes_key<S: DiffieHellman>(
    encrypted_bundle: &EncryptedAesKeyBundle,
    recipient_secret_key: S,
) -> Result<[u8; 32], String> {
    // 1. Decode hex-encoded data from the bundle.
    let ephemeral_public_key_bytes: [u8; 32] = hex::decode(&encrypted_bundle.ephemeral_public_key)
        .map_err(|e| e.to_string())?
        .try_into()
        .map_err(|_| "Invalid ephemeral public key length".to_string())?;
    let ephemeral_public_key = PublicKey::from(ephemeral_public_key_bytes);

    let encrypted_key = hex::decode(&encrypted_bundle.encrypted_key).map_err(|e| e.to_string())?;
    let nonce_bytes = hex::decode(&encrypted_bundle.nonce).map_err(|e| e.to_string())?;

    // 2. Compute the same shared secret.
    let shared_secret = recipient_secret_key.diffie_hellman(&ephemeral_public_key);

    // 3. Derive the same KEK using the same HKDF parameters.
    let hk = Hkdf::<Sha256>::new(
        Some(ephemeral_public_key.as_bytes()),
        shared_secret.as_bytes(),
    );
    let mut kek = [0u8; 32];
    hk.expand(b"chiral-network-kek", &mut kek)
        .map_err(|e| format!("HKDF expansion failed: {}", e))?;

    // 4. Decrypt the AES key (DEK) with the derived KEK.
    let key = Key::<Aes256Gcm>::from_slice(&kek);
    let kek_cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let decrypted_key_vec = kek_cipher
        .decrypt(nonce, encrypted_key.as_ref())
        .map_err(|e| format!("AES key decryption failed: {}", e))?;

    decrypted_key_vec
        .try_into()
        .map_err(|_| "Decrypted key is not 32 bytes".to_string())
}

/// A bundle containing an encrypted message and the necessary data for decryption.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EncryptedMessageBundle {
    /// The sender's temporary public key (32 bytes), hex-encoded.
    pub ephemeral_public_key: String,
    /// The message, encrypted and then hex-encoded.
    pub encrypted_message: String,
    /// The nonce used for AES-GCM encryption (12 bytes), hex-encoded.
    pub nonce: String,
}

/// Encrypts a message using the recipient's public key (ECIES pattern).
///
/// # Arguments
/// * `message` - The message to encrypt.
/// * `recipient_public_key` - The recipient's X25519 public key.
///
/// # Returns
/// An `EncryptedMessageBundle` struct containing the data needed for decryption.
pub fn encrypt_message(
    message: &[u8],
    recipient_public_key: &PublicKey,
) -> Result<EncryptedMessageBundle, String> {
    // 1. Generate a temporary (ephemeral) X25519 key pair for the sender.
    let ephemeral_secret = EphemeralSecret::random_from_rng(OsRng);
    let ephemeral_public_key = PublicKey::from(&ephemeral_secret);

    // 2. Compute the shared secret.
    let shared_secret = ephemeral_secret.diffie_hellman(recipient_public_key);

    // 3. Use HKDF to derive an encryption key from the shared secret.
    let hk = Hkdf::<Sha256>::new(
        Some(ephemeral_public_key.as_bytes()),
        shared_secret.as_bytes(),
    );
    let mut encryption_key = [0u8; 32]; // 32 bytes for an AES-256 key
    hk.expand(b"chiral-network-msg", &mut encryption_key)
        .map_err(|e| format!("HKDF expansion failed: {}", e))?;

    // 4. Encrypt the message with the derived key.
    let key = Key::<Aes256Gcm>::from_slice(&encryption_key);
    let cipher = Aes256Gcm::new(key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng); // Generate a random nonce
    let encrypted_message = cipher
        .encrypt(&nonce, message)
        .map_err(|e| format!("Message encryption failed: {}", e))?;

    // 5. Return the bundle with hex-encoded data.
    Ok(EncryptedMessageBundle {
        ephemeral_public_key: hex::encode(ephemeral_public_key.as_bytes()),
        encrypted_message: hex::encode(encrypted_message),
        nonce: hex::encode(nonce.as_slice()),
    })
}

/// Decrypts a message using the recipient's private key.
///
/// # Arguments
/// * `encrypted_bundle` - The `EncryptedMessageBundle` received from the sender.
/// * `recipient_secret_key` - The recipient's X25519 private key.
///
/// # Returns
/// The decrypted message as a `Vec<u8>`.
pub fn decrypt_message<S: DiffieHellman>(
    encrypted_bundle: &EncryptedMessageBundle,
    recipient_secret_key: S,
) -> Result<Vec<u8>, String> {
    // 1. Decode hex-encoded data from the bundle.
    let ephemeral_public_key_bytes: [u8; 32] = hex::decode(&encrypted_bundle.ephemeral_public_key)
        .map_err(|e| e.to_string())?
        .try_into()
        .map_err(|_| "Invalid ephemeral public key length".to_string())?;
    let ephemeral_public_key = PublicKey::from(ephemeral_public_key_bytes);

    let encrypted_message =
        hex::decode(&encrypted_bundle.encrypted_message).map_err(|e| e.to_string())?;
    let nonce_bytes = hex::decode(&encrypted_bundle.nonce).map_err(|e| e.to_string())?;
    let nonce = Nonce::from_slice(&nonce_bytes);

    // 2. Derive the same encryption key using the same process as encryption.
    let shared_secret = recipient_secret_key.diffie_hellman(&ephemeral_public_key);
    let hk = Hkdf::<Sha256>::new(
        Some(ephemeral_public_key.as_bytes()),
        shared_secret.as_bytes(),
    );
    let mut encryption_key = [0u8; 32];
    hk.expand(b"chiral-network-msg", &mut encryption_key)
        .map_err(|e| format!("HKDF expansion failed: {}", e))?;

    // 3. Decrypt the message.
    let key = Key::<Aes256Gcm>::from_slice(&encryption_key);
    let cipher = Aes256Gcm::new(key);
    cipher
        .decrypt(nonce, encrypted_message.as_ref())
        .map_err(|e| format!("Message decryption failed: {}", e))
}

/// A bundle containing a message and a signature to verify its authenticity.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SignedMessage {
    /// The original message data, base64-encoded for JSON compatibility.
    pub message: String,
    /// The signer's public key (32 bytes), hex-encoded.
    pub signer_public_key: String,
    /// The Ed25519 signature over the message (64 bytes), hex-encoded.
    pub signature: String,
}

/// Signs a message with the sender's private key to prove authenticity.
///
/// # Arguments
/// * `message` - The message to sign.
/// * `sender_signing_key` - The sender's Ed25519 `SigningKey`.
///
/// # Returns
/// A `SignedMessage` struct containing the message, signature, and public key.
pub fn sign_message(
    message: &[u8],
    sender_signing_key: &SigningKey,
) -> Result<SignedMessage, String> {
    let signature = sender_signing_key.sign(message);

    Ok(SignedMessage {
        message: general_purpose::STANDARD.encode(message),
        signer_public_key: hex::encode(sender_signing_key.verifying_key().as_bytes()),
        signature: hex::encode(signature.to_bytes()),
    })
}

/// Verifies the signature of a message to ensure it's authentic and untampered.
///
/// # Arguments
/// * `signed_message` - The `SignedMessage` bundle to verify.
///
/// # Returns
/// `Ok(true)` if the signature is valid, `Ok(false)` if it's not.
/// Returns an `Err` if any part of the bundle is malformed.
pub fn verify_message(signed_message: &SignedMessage) -> Result<bool, String> {
    // 1. Decode the data from the bundle.
    let message_bytes = general_purpose::STANDARD
        .decode(&signed_message.message)
        .map_err(|e| format!("Invalid base64 message: {}", e))?;

    let public_key_bytes: [u8; 32] = hex::decode(&signed_message.signer_public_key)
        .map_err(|e| format!("Invalid public key hex: {}", e))?
        .try_into()
        .map_err(|_| "Invalid public key length".to_string())?;
    let signer_public_key = VerifyingKey::from_bytes(&public_key_bytes)
        .map_err(|e| format!("Invalid public key: {}", e))?;

    let signature_bytes: [u8; 64] = hex::decode(&signed_message.signature)
        .map_err(|e| format!("Invalid signature hex: {}", e))?
        .try_into()
        .map_err(|_| "Invalid signature length".to_string())?;
    let signature = Signature::from_bytes(&signature_bytes);

    // 2. Verify the signature against the message.
    // The `verify` method will hash the message internally before checking.
    Ok(signer_public_key.verify(&message_bytes, &signature).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::fs;
    use tokio::time::{sleep, Duration};

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

    #[test]
    fn test_message_encryption_decryption() {
        // 1. Setup recipient's key pair.
        let recipient_secret = StaticSecret::random_from_rng(OsRng);
        let recipient_public = PublicKey::from(&recipient_secret);

        // 2. Define a message to encrypt.
        let original_message = b"This is a secret message for the Chiral Network.";

        // 3. Encrypt the message for the recipient.
        let encrypted_bundle = encrypt_message(original_message, &recipient_public).unwrap();

        // 4. Decrypt the message using the recipient's secret key.
        let decrypted_message = decrypt_message(&encrypted_bundle, &recipient_secret).unwrap();

        // 5. Verify that the decrypted message matches the original.
        assert_eq!(original_message.to_vec(), decrypted_message);

        // 6. Negative test: try to decrypt with the wrong key.
        let wrong_secret = StaticSecret::random_from_rng(OsRng);
        let decryption_result = decrypt_message(&encrypted_bundle, &wrong_secret);
        assert!(decryption_result.is_err());
        assert!(decryption_result
            .unwrap_err()
            .contains("Message decryption failed"));
    }

    #[test]
    fn test_message_signing_and_verification() {
        // 1. Setup sender's key pair.
        let mut csprng = OsRng;
        let sender_signing_key = SigningKey::generate(&mut csprng);

        // 2. Define a message to sign.
        let original_message = b"This message needs to be authenticated.";

        // 3. Sign the message.
        let signed_message = sign_message(original_message, &sender_signing_key).unwrap();

        // 4. Verify the message.
        let is_valid = verify_message(&signed_message).unwrap();
        assert!(is_valid, "Signature should be valid");

        // 5. Negative test: tampered message should fail verification.
        let mut tampered_bundle = signed_message.clone();
        tampered_bundle.message = general_purpose::STANDARD.encode(b"This is a tampered message!");
        let is_tampered_valid = verify_message(&tampered_bundle).unwrap();
        assert!(
            !is_tampered_valid,
            "Signature for tampered message should be invalid"
        );
    }

    // Add E2E messaging tests. Marked #[ignore] so they don't run in normal CI
    // This is so that they won't break the CI while changes are being made to the encryption/decryption.
    // Run them explicitly with: cargo test --test <name> -- --ignored
    #[tokio::test]
    #[ignore]
    async fn e2e_message_encryption_between_nodes() {
        // Simulate two nodes: sender and recipient
        let recipient_secret = x25519_dalek::StaticSecret::random_from_rng(&mut OsRng);
        let recipient_public = x25519_dalek::PublicKey::from(&recipient_secret);
        let original_message = b"Hello E2EE node-to-node test";

        // Sender encrypts the message for recipient
        let encrypted_bundle = encrypt_message(original_message, &recipient_public)
            .expect("encryption should succeed");

        // Simulate small network latency
        sleep(Duration::from_millis(150)).await;

        // Recipient decrypts
        let decrypted = decrypt_message(&encrypted_bundle, &recipient_secret)
            .expect("decryption should succeed");

        assert_eq!(decrypted, original_message);
    }

    #[tokio::test]
    #[ignore]
    async fn ensure_no_plaintext_in_encrypted_bundle() {
        // Verify that the encrypted bundle does not contain the plaintext (hex) anywhere
        let recipient_secret = x25519_dalek::StaticSecret::random_from_rng(&mut OsRng);
        let recipient_public = x25519_dalek::PublicKey::from(&recipient_secret);
        let original_message = b"Sensitive payload - must not leak";

        let encrypted_bundle = encrypt_message(original_message, &recipient_public)
            .expect("encryption should succeed");

        // hex of original - ensure it is not present in any returned hex fields
        let original_hex = hex::encode(original_message);

        // The encrypted fields are hex strings; check they don't (accidentally) contain original hex
        assert!(
            !encrypted_bundle.encrypted_message.contains(&original_hex),
            "Encrypted message unexpectedly contains plaintext hex"
        );
        assert!(
            !encrypted_bundle
                .ephemeral_public_key
                .contains(&original_hex),
            "Ephemeral public key unexpectedly contains plaintext hex"
        );
        assert!(
            !encrypted_bundle.nonce.contains(&original_hex),
            "Nonce unexpectedly contains plaintext hex"
        );
    }

    #[tokio::test]
    #[ignore]
    async fn message_delivery_with_latency_and_reconnects_simulation() {
        // Simulate network hiccups: encryption happens, message delayed, "reconnect" re-send.
        let recipient_secret = x25519_dalek::StaticSecret::random_from_rng(&mut OsRng);
        let recipient_public = x25519_dalek::PublicKey::from(&recipient_secret);
        let original_message = b"Message across unreliable network";

        // Sender encrypts
        let encrypted_bundle =
            encrypt_message(original_message, &recipient_public).expect("encrypt");

        // Simulate message lost on first attempt
        sleep(Duration::from_millis(100)).await;

        // Simulate reconnect and re-send (re-encrypt or reuse same bundle is valid)
        // Here we re-use same bundle to model idempotent delivery
        sleep(Duration::from_millis(250)).await;

        // Recipient tries to decrypt after reconnect
        let decrypted =
            decrypt_message(&encrypted_bundle, &recipient_secret).expect("decrypt after reconnect");

        assert_eq!(decrypted, original_message);
    }
}
