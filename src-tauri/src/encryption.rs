//! End-to-End Encryption Module for Chiral Network V2
//!
//! Provides secure file encryption using:
//! - X25519 for key exchange (ECDH)
//! - AES-256-GCM for symmetric encryption
//! - HKDF-SHA256 for key derivation
//!
//! This implements the ECIES (Elliptic Curve Integrated Encryption Scheme) pattern.

use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use x25519_dalek::{EphemeralSecret, PublicKey, StaticSecret};

/// Domain separator for key derivation
const HKDF_INFO: &[u8] = b"chiral-network-v2-e2ee";

/// Encrypted file bundle containing all data needed for decryption
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptedFileBundle {
    /// Sender's ephemeral public key (32 bytes, hex-encoded)
    pub ephemeral_public_key: String,
    /// Encrypted file data (hex-encoded)
    pub ciphertext: String,
    /// Nonce used for AES-GCM (12 bytes, hex-encoded)
    pub nonce: String,
}

/// User's encryption keypair for receiving encrypted files
#[derive(Clone)]
pub struct EncryptionKeypair {
    secret: StaticSecret,
    public: PublicKey,
}

impl EncryptionKeypair {
    /// Generate a new random keypair
    pub fn generate() -> Self {
        let secret = StaticSecret::random_from_rng(OsRng);
        let public = PublicKey::from(&secret);
        Self { secret, public }
    }

    /// Create keypair from existing secret key bytes
    pub fn from_secret_bytes(bytes: [u8; 32]) -> Self {
        let secret = StaticSecret::from(bytes);
        let public = PublicKey::from(&secret);
        Self { secret, public }
    }

    /// Derive keypair from wallet private key (for deterministic key generation)
    pub fn from_wallet_key(wallet_private_key: &[u8]) -> Self {
        use sha2::Digest;
        let mut hasher = Sha256::new();
        hasher.update(b"chiral-encryption-key-derivation");
        hasher.update(wallet_private_key);
        let hash = hasher.finalize();
        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&hash);
        Self::from_secret_bytes(key_bytes)
    }

    /// Get the public key as hex string (for sharing with others)
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.public.as_bytes())
    }

    /// Get the public key bytes
    pub fn public_key_bytes(&self) -> [u8; 32] {
        *self.public.as_bytes()
    }

    /// Get the secret key bytes (be careful with this!)
    pub fn secret_key_bytes(&self) -> [u8; 32] {
        self.secret.to_bytes()
    }
}

/// Encrypt data for a recipient using their public key
///
/// # Arguments
/// * `plaintext` - The data to encrypt
/// * `recipient_public_key` - Recipient's X25519 public key (32 bytes)
///
/// # Returns
/// An `EncryptedFileBundle` containing all data needed for decryption
pub fn encrypt_for_recipient(
    plaintext: &[u8],
    recipient_public_key: &[u8; 32],
) -> Result<EncryptedFileBundle, String> {
    let recipient_pk = PublicKey::from(*recipient_public_key);

    // Generate ephemeral keypair for this encryption
    let ephemeral_secret = EphemeralSecret::random_from_rng(OsRng);
    let ephemeral_public = PublicKey::from(&ephemeral_secret);

    // Compute shared secret via ECDH
    let shared_secret = ephemeral_secret.diffie_hellman(&recipient_pk);

    // Derive encryption key using HKDF
    let hk = Hkdf::<Sha256>::new(Some(ephemeral_public.as_bytes()), shared_secret.as_bytes());
    let mut encryption_key = [0u8; 32];
    hk.expand(HKDF_INFO, &mut encryption_key)
        .map_err(|e| format!("HKDF expansion failed: {}", e))?;

    // Encrypt with AES-256-GCM
    let key = Key::<Aes256Gcm>::from_slice(&encryption_key);
    let cipher = Aes256Gcm::new(key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|e| format!("Encryption failed: {}", e))?;

    Ok(EncryptedFileBundle {
        ephemeral_public_key: hex::encode(ephemeral_public.as_bytes()),
        ciphertext: hex::encode(ciphertext),
        nonce: hex::encode(nonce.as_slice()),
    })
}

/// Decrypt data using the recipient's keypair
///
/// # Arguments
/// * `bundle` - The encrypted file bundle
/// * `keypair` - Recipient's encryption keypair
///
/// # Returns
/// The decrypted plaintext
pub fn decrypt_with_keypair(
    bundle: &EncryptedFileBundle,
    keypair: &EncryptionKeypair,
) -> Result<Vec<u8>, String> {
    // Decode hex data
    let ephemeral_pk_bytes: [u8; 32] = hex::decode(&bundle.ephemeral_public_key)
        .map_err(|e| format!("Invalid ephemeral public key: {}", e))?
        .try_into()
        .map_err(|_| "Ephemeral public key must be 32 bytes")?;

    let ciphertext = hex::decode(&bundle.ciphertext)
        .map_err(|e| format!("Invalid ciphertext: {}", e))?;

    let nonce_bytes = hex::decode(&bundle.nonce)
        .map_err(|e| format!("Invalid nonce: {}", e))?;

    let ephemeral_pk = PublicKey::from(ephemeral_pk_bytes);

    // Compute shared secret via ECDH
    let shared_secret = keypair.secret.diffie_hellman(&ephemeral_pk);

    // Derive the same encryption key using HKDF
    let hk = Hkdf::<Sha256>::new(Some(ephemeral_pk.as_bytes()), shared_secret.as_bytes());
    let mut encryption_key = [0u8; 32];
    hk.expand(HKDF_INFO, &mut encryption_key)
        .map_err(|e| format!("HKDF expansion failed: {}", e))?;

    // Decrypt with AES-256-GCM
    let key = Key::<Aes256Gcm>::from_slice(&encryption_key);
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(&nonce_bytes);

    cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|e| format!("Decryption failed: {}", e))
}

/// Convenience function to encrypt for a recipient given their public key as hex
pub fn encrypt_for_recipient_hex(
    plaintext: &[u8],
    recipient_public_key_hex: &str,
) -> Result<EncryptedFileBundle, String> {
    let pk_bytes: [u8; 32] = hex::decode(recipient_public_key_hex)
        .map_err(|e| format!("Invalid public key hex: {}", e))?
        .try_into()
        .map_err(|_| "Public key must be 32 bytes")?;

    encrypt_for_recipient(plaintext, &pk_bytes)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keypair_generation() {
        let keypair = EncryptionKeypair::generate();
        assert_eq!(keypair.public_key_hex().len(), 64); // 32 bytes = 64 hex chars
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let recipient = EncryptionKeypair::generate();
        let plaintext = b"Hello, Chiral Network!";

        let bundle = encrypt_for_recipient(plaintext, &recipient.public_key_bytes()).unwrap();
        let decrypted = decrypt_with_keypair(&bundle, &recipient).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_decrypt_large_data() {
        let recipient = EncryptionKeypair::generate();
        let plaintext: Vec<u8> = (0..100_000).map(|i| (i % 256) as u8).collect();

        let bundle = encrypt_for_recipient(&plaintext, &recipient.public_key_bytes()).unwrap();
        let decrypted = decrypt_with_keypair(&bundle, &recipient).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_wrong_key_fails() {
        let sender_target = EncryptionKeypair::generate();
        let wrong_recipient = EncryptionKeypair::generate();
        let plaintext = b"Secret message";

        let bundle = encrypt_for_recipient(plaintext, &sender_target.public_key_bytes()).unwrap();
        let result = decrypt_with_keypair(&bundle, &wrong_recipient);

        assert!(result.is_err());
    }

    #[test]
    fn test_wallet_derived_keypair() {
        let wallet_key = [0u8; 32]; // Mock wallet private key
        let keypair1 = EncryptionKeypair::from_wallet_key(&wallet_key);
        let keypair2 = EncryptionKeypair::from_wallet_key(&wallet_key);

        // Same wallet key should produce same encryption keypair
        assert_eq!(keypair1.public_key_hex(), keypair2.public_key_hex());
    }

    #[test]
    fn test_different_wallet_keys_produce_different_keypairs() {
        let wallet_key1 = [0u8; 32];
        let wallet_key2 = [1u8; 32];
        let keypair1 = EncryptionKeypair::from_wallet_key(&wallet_key1);
        let keypair2 = EncryptionKeypair::from_wallet_key(&wallet_key2);

        assert_ne!(keypair1.public_key_hex(), keypair2.public_key_hex());
    }

    #[test]
    fn test_encrypt_decrypt_hex_roundtrip() {
        let recipient = EncryptionKeypair::generate();
        let plaintext = b"Testing hex API";
        let pub_key_hex = recipient.public_key_hex();

        let bundle = encrypt_for_recipient_hex(plaintext, &pub_key_hex).unwrap();
        let decrypted = decrypt_with_keypair(&bundle, &recipient).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_decrypt_hex_invalid_key() {
        let result = encrypt_for_recipient_hex(b"test", "not_valid_hex");
        assert!(result.is_err());
    }

    #[test]
    fn test_encrypt_decrypt_hex_wrong_length() {
        let result = encrypt_for_recipient_hex(b"test", "aabb");
        assert!(result.is_err());
    }

    #[test]
    fn test_encrypted_bundle_fields_are_hex() {
        let recipient = EncryptionKeypair::generate();
        let bundle = encrypt_for_recipient(b"data", &recipient.public_key_bytes()).unwrap();

        // All fields should be valid hex strings
        assert!(hex::decode(&bundle.ephemeral_public_key).is_ok());
        assert!(hex::decode(&bundle.ciphertext).is_ok());
        assert!(hex::decode(&bundle.nonce).is_ok());

        // Ephemeral public key should be 32 bytes (64 hex chars)
        assert_eq!(bundle.ephemeral_public_key.len(), 64);

        // Nonce should be 12 bytes (24 hex chars) for AES-GCM
        assert_eq!(bundle.nonce.len(), 24);

        // Ciphertext should be longer than plaintext (includes auth tag)
        assert!(hex::decode(&bundle.ciphertext).unwrap().len() > 4);
    }

    #[test]
    fn test_encrypt_empty_data() {
        let recipient = EncryptionKeypair::generate();
        let plaintext = b"";

        let bundle = encrypt_for_recipient(plaintext, &recipient.public_key_bytes()).unwrap();
        let decrypted = decrypt_with_keypair(&bundle, &recipient).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_from_secret_bytes() {
        let bytes = [42u8; 32];
        let keypair1 = EncryptionKeypair::from_secret_bytes(bytes);
        let keypair2 = EncryptionKeypair::from_secret_bytes(bytes);

        assert_eq!(keypair1.public_key_hex(), keypair2.public_key_hex());
        assert_eq!(keypair1.secret_key_bytes(), keypair2.secret_key_bytes());
    }

    #[test]
    fn test_public_key_bytes_roundtrip() {
        let keypair = EncryptionKeypair::generate();
        let pk_bytes = keypair.public_key_bytes();
        let pk_hex = keypair.public_key_hex();

        assert_eq!(hex::encode(pk_bytes), pk_hex);
    }

    #[test]
    fn test_each_encryption_produces_unique_ciphertext() {
        let recipient = EncryptionKeypair::generate();
        let plaintext = b"Same message";

        let bundle1 = encrypt_for_recipient(plaintext, &recipient.public_key_bytes()).unwrap();
        let bundle2 = encrypt_for_recipient(plaintext, &recipient.public_key_bytes()).unwrap();

        // Each encryption uses ephemeral keys and random nonces
        assert_ne!(bundle1.ephemeral_public_key, bundle2.ephemeral_public_key);
        assert_ne!(bundle1.nonce, bundle2.nonce);
        assert_ne!(bundle1.ciphertext, bundle2.ciphertext);

        // But both should decrypt to the same plaintext
        let d1 = decrypt_with_keypair(&bundle1, &recipient).unwrap();
        let d2 = decrypt_with_keypair(&bundle2, &recipient).unwrap();
        assert_eq!(d1, d2);
    }

    #[test]
    fn test_bundle_serialization_roundtrip() {
        let recipient = EncryptionKeypair::generate();
        let bundle = encrypt_for_recipient(b"serialize me", &recipient.public_key_bytes()).unwrap();

        let json = serde_json::to_string(&bundle).unwrap();
        let deserialized: EncryptedFileBundle = serde_json::from_str(&json).unwrap();

        assert_eq!(bundle.ephemeral_public_key, deserialized.ephemeral_public_key);
        assert_eq!(bundle.ciphertext, deserialized.ciphertext);
        assert_eq!(bundle.nonce, deserialized.nonce);

        // Decryption should still work with deserialized bundle
        let decrypted = decrypt_with_keypair(&deserialized, &recipient).unwrap();
        assert_eq!(decrypted, b"serialize me");
    }
}
