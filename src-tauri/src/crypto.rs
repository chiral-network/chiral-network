use aes_gcm::{aead::{Aead, AeadCore, OsRng}, Aes256Gcm, KeyInit, Nonce};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use x25519_dalek::{EphemeralSecret, PublicKey, SharedSecret, StaticSecret};

/// A bundle containing the encrypted AES key and the necessary data for decryption.
/// This struct is designed to be serialized (e.g., to JSON) and stored as file metadata.
#[derive(Serialize, Deserialize, Debug)]
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
    let hk = Hkdf::<Sha256>::new(Some(ephemeral_public_key.as_bytes()), shared_secret.as_bytes());
    let mut kek = [0u8; 32]; // 32 bytes for an AES-256 key
    hk.expand(b"chiral-network-kek", &mut kek)
        .map_err(|e| format!("HKDF expansion failed: {}", e))?;

    // 4. Encrypt the AES key (DEK) with the derived KEK.
    let kek_cipher = Aes256Gcm::new_from_slice(&kek).map_err(|e| e.to_string())?;
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
    let hk = Hkdf::<Sha256>::new(Some(ephemeral_public_key.as_bytes()), shared_secret.as_bytes());
    let mut kek = [0u8; 32];
    hk.expand(b"chiral-network-kek", &mut kek)
        .map_err(|e| format!("HKDF expansion failed: {}", e))?;

    // 4. Decrypt the AES key (DEK) with the derived KEK.
    let kek_cipher = Aes256Gcm::new_from_slice(&kek).map_err(|e| e.to_string())?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let decrypted_key_vec = kek_cipher
        .decrypt(nonce, encrypted_key.as_ref())
        .map_err(|e| format!("AES key decryption failed: {}", e))?;

    decrypted_key_vec
        .try_into()
        .map_err(|_| "Decrypted key is not 32 bytes".to_string())
}