use sha2::{Sha256, Digest};
use aes_gcm::{Aes256Gcm, Key, Nonce, KeyInit};
use aes_gcm::aead::{Aead, OsRng};
use rand::RngCore;
use std::fs::{File, self};
use std::io::{Read, Error, Write};
use std::path::{Path, PathBuf};
use x25519_dalek::PublicKey;

// Import the new crypto functions and the bundle struct
use crate::crypto::{encrypt_aes_key, EncryptedAesKeyBundle};

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct ChunkInfo {
    pub index: u32,
    pub hash: String,
    pub size: usize,
    pub encrypted_size: usize,
}

pub struct ChunkManager {
    chunk_size: usize,
    storage_path: PathBuf,
}

impl ChunkManager {
    pub fn new(storage_path: PathBuf) -> Self {
        ChunkManager {
            chunk_size: 256 * 1024, // 256KB
            storage_path,
        }
    }

    // The function now takes the recipient's public key and returns the encrypted key bundle
    pub fn chunk_and_encrypt_file(
        &self,
        file_path: &Path,
        recipient_public_key: &PublicKey,
    ) -> Result<(Vec<ChunkInfo>, EncryptedAesKeyBundle), String> {
        let mut key_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut key_bytes);
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);

        let mut file = File::open(file_path).map_err(|e| e.to_string())?;
        let mut chunks = Vec::new();
        let mut buffer = vec![0u8; self.chunk_size];
        let mut index = 0;

        loop {
            let bytes_read = file.read(&mut buffer).map_err(|e| e.to_string())?;
            if bytes_read == 0 { break; }

            let chunk_data = &buffer[..bytes_read];
            let chunk_hash = self.hash_chunk(chunk_data);
            
            // The nonce is now prepended to the ciphertext by `encrypt_chunk`
            let encrypted_data_with_nonce = self.encrypt_chunk(chunk_data, &key)?;

            chunks.push(ChunkInfo {
                index,
                hash: chunk_hash.clone(),
                size: bytes_read,
                encrypted_size: encrypted_data_with_nonce.len(),
            });

            self.save_chunk(&chunk_hash, &encrypted_data_with_nonce).map_err(|e| e.to_string())?;
            index += 1;
        }

        // Instead of returning the raw key, encrypt it with the recipient's public key
        let encrypted_key_bundle = encrypt_aes_key(&key_bytes, recipient_public_key)?;

        Ok((chunks, encrypted_key_bundle))
    }

    // This function now returns the nonce and ciphertext combined for easier storage
    fn encrypt_chunk(&self, data: &[u8], key: &Key<Aes256Gcm>) -> Result<Vec<u8>, String> {
        let cipher = Aes256Gcm::new(key);
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng); // Generate a unique nonce for each chunk

        let ciphertext = cipher.encrypt(&nonce, data).map_err(|e| e.to_string())?;
        let mut result = nonce.to_vec();
        result.extend_from_slice(&ciphertext);
        Ok(result)
    }

    fn hash_chunk(&self, data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }

    // This function now saves the combined [nonce][ciphertext] blob
    fn save_chunk(&self, hash: &str, data_with_nonce: &[u8]) -> Result<(), Error> {
        fs::create_dir_all(&self.storage_path)?;
        fs::write(self.storage_path.join(hash), data_with_nonce)?;
        Ok(())
    }

    pub fn hash_file(&self, file_path: &Path) -> Result<String, Error> {
        let mut file = File::open(file_path)?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0; 1024 * 1024]; // 1MB buffer on the heap

        loop {
            let bytes_read = file.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }
        Ok(format!("{:x}", hasher.finalize()))
    }
}
