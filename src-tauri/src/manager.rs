use sha2::{Sha256, Digest};
use rs_merkle::{MerkleTree, Hasher};
use aes_gcm::{Aes256Gcm, Key, Nonce, KeyInit};
use aes_gcm::aead::{Aead, AeadCore, OsRng};
use rand::RngCore;
use std::fs::{File, self};
use std::io::{Read, Error, Write};
use std::path::{Path, PathBuf};
use reed_solomon_erasure::galois_8::ReedSolomon;
use x25519_dalek::PublicKey;
use std::sync::Mutex;

// Import the new encryption functions and the bundle struct
use crate::crypto::{decrypt_aes_key, encrypt_aes_key, EncryptedAesKeyBundle, DiffieHellman};

use std::collections::HashMap;
use lazy_static::lazy_static;

// Simple thread-safe LRU cache implementation
const L1_CACHE_CAPACITY: usize = 128;

struct LruCache {
    map: HashMap<String, Vec<u8>>,
    order: Vec<String>,
    capacity: usize,
}

impl LruCache {
    fn new(capacity: usize) -> Self {
        LruCache {
            map: HashMap::new(),
            order: Vec::new(),
            capacity,
        }
    }

    fn get(&mut self, key: &str) -> Option<Vec<u8>> {
        if let Some(value) = self.map.get(key) {
            // Move key to the end (most recently used)
            self.order.retain(|k| k != key);
            self.order.push(key.to_string());
            Some(value.clone())
        } else {
            None
        }
    }

    fn put(&mut self, key: String, value: Vec<u8>) {
        if self.map.contains_key(&key) {
            self.order.retain(|k| k != &key);
        }
        self.order.push(key.clone());
        self.map.insert(key.clone(), value);

        // Evict least recently used if over capacity
        if self.order.len() > self.capacity {
            if let Some(lru) = self.order.first() {
                self.map.remove(lru);
            }
            self.order.remove(0);
        }
    }
}

lazy_static! {
    static ref L1_CACHE: Mutex<LruCache> = Mutex::new(LruCache::new(L1_CACHE_CAPACITY));
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct ChunkInfo {
    pub index: u32,
    pub hash: String,
    pub size: usize,
    pub shards: Vec<String>, // Hashes of the Reed-Solomon shards
    pub encrypted_size: usize,
}

/// Contains all metadata required to find, verify, and decrypt a file.
/// This manifest should be saved by the uploader and securely sent to the recipient.
#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct FileManifest {
    /// The Merkle root of all original chunk hashes. This is the file's unique identifier.
    pub merkle_root: String,
    /// Information about each chunk needed for reassembly.
    pub chunks: Vec<ChunkInfo>,
    /// The encrypted AES key bundle needed for decryption.
    pub encrypted_key_bundle: EncryptedAesKeyBundle,
}

/// A simple Sha256 hasher implementation for the Merkle tree.
#[derive(Clone)]
pub struct Sha256Hasher;

impl Hasher for Sha256Hasher {
    type Hash = [u8; 32];

    fn hash(data: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(data);
        hasher.finalize().into()
    }
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
    ) -> Result<FileManifest, String> {
        const DATA_SHARDS: usize = 10;
        const PARITY_SHARDS: usize = 4;
        let r = ReedSolomon::new(DATA_SHARDS, PARITY_SHARDS).unwrap();

        let mut key_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut key_bytes);
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);

        let mut file = File::open(file_path).map_err(|e| e.to_string())?;
        let mut chunks_info = Vec::new();
        let mut chunk_hashes: Vec<[u8; 32]> = Vec::new();
        let mut buffer = vec![0u8; self.chunk_size];
        let mut index = 0;

        loop {
            let bytes_read = file.read(&mut buffer).map_err(|e| e.to_string())?;
            if bytes_read == 0 { break; }

            let chunk_data = &buffer[..bytes_read];
            let chunk_hash_bytes = Sha256Hasher::hash(chunk_data);
            chunk_hashes.push(chunk_hash_bytes);
            let chunk_hash_hex = hex::encode(chunk_hash_bytes);

            // Pad the chunk data to be a multiple of DATA_SHARDS
            let mut padded_chunk_data = chunk_data.to_vec();
            let remainder = padded_chunk_data.len() % DATA_SHARDS;
            if remainder != 0 {
                padded_chunk_data.resize(padded_chunk_data.len() + DATA_SHARDS - remainder, 0);
            }

            // Create shards
            let mut shards: Vec<Vec<u8>> = padded_chunk_data
                .chunks(padded_chunk_data.len() / DATA_SHARDS)
                .map(|c| c.to_vec())
                .collect();

            // Encode the shards
            r.encode(&mut shards).unwrap();

            let mut shard_hashes = Vec::new();
            let mut total_encrypted_size = 0;

            for shard in shards {
                let encrypted_shard_with_nonce = self.encrypt_chunk(&shard, &key)?;
                let shard_hash = self.hash_chunk(&encrypted_shard_with_nonce);
                self.save_chunk(&shard_hash, &encrypted_shard_with_nonce).map_err(|e| e.to_string())?;
                shard_hashes.push(shard_hash);
                total_encrypted_size += encrypted_shard_with_nonce.len();
            }

            chunks_info.push(ChunkInfo {
                index,
                hash: chunk_hash_hex.clone(),
                size: bytes_read,
                shards: shard_hashes,
                encrypted_size: total_encrypted_size,
            });

            index += 1;
        }

        // Build the Merkle tree from the chunk hashes to get the root hash.
        let merkle_tree = MerkleTree::<Sha256Hasher>::from_leaves(&chunk_hashes);
        let merkle_root = merkle_tree.root().ok_or("Failed to compute Merkle root")?;

        // Encrypt the file's AES key with the recipient's public key.
        let encrypted_key_bundle = encrypt_aes_key(&key_bytes, recipient_public_key)?;

        Ok(FileManifest {
            merkle_root: hex::encode(merkle_root),
            chunks: chunks_info,
            encrypted_key_bundle,
        })
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
        // Prime the L1 cache
        {
            let mut cache = L1_CACHE.lock().unwrap();
            cache.put(hash.to_string(), data_with_nonce.to_vec());
        }
        Ok(())
    }

    pub fn read_chunk(&self, hash: &str) -> Result<Vec<u8>, Error> {
        // Check L1 cache first
        {
            let mut cache = L1_CACHE.lock().unwrap();
            if let Some(data) = cache.get(hash) {
                return Ok(data.clone());
            }
        }
        // Fallback to disk
        let data = fs::read(self.storage_path.join(hash))?;
        // Populate L1 cache
        {
            let mut cache = L1_CACHE.lock().unwrap();
            cache.put(hash.to_string(), data.clone());
        }
        Ok(data)
    }

    fn decrypt_chunk(&self, data_with_nonce: &[u8], key: &Key<Aes256Gcm>) -> Result<Vec<u8>, String> {
        let cipher = Aes256Gcm::new(key);
        // AES-GCM nonce is 12 bytes. The nonce is prepended to the ciphertext.
        if data_with_nonce.len() < 12 {
            return Err("Encrypted data is too short to contain a nonce".to_string());
        }
        let (nonce_bytes, ciphertext) = data_with_nonce.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| format!("Chunk decryption failed: {}", e))
    }

    pub fn reassemble_and_decrypt_file<S: DiffieHellman>(
        &self,
        chunks: &[ChunkInfo],
        output_path: &Path,
        encrypted_key_bundle: &EncryptedAesKeyBundle,
        recipient_secret_key: S,
    ) -> Result<(), String> {
        const DATA_SHARDS: usize = 10;
        const PARITY_SHARDS: usize = 4;
        let r = ReedSolomon::new(DATA_SHARDS, PARITY_SHARDS).unwrap();

        let key_bytes = decrypt_aes_key(encrypted_key_bundle, recipient_secret_key)?;
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);

        let mut output_file = File::create(output_path).map_err(|e| e.to_string())?;

        // Assuming chunks are ordered by index. If not, they should be sorted first.
        for chunk_info in chunks {
            let mut shards: Vec<Option<Vec<u8>>> = Vec::with_capacity(DATA_SHARDS + PARITY_SHARDS);
            for shard_hash in &chunk_info.shards {
                if let Ok(encrypted_shard_with_nonce) = self.read_chunk(shard_hash) {
                    if let Ok(decrypted_shard) = self.decrypt_chunk(&encrypted_shard_with_nonce, &key) {
                        shards.push(Some(decrypted_shard));
                    } else {
                        shards.push(None);
                    }
                } else {
                    shards.push(None);
                }
            }

            // Reconstruct the original data
            r.reconstruct(&mut shards).map_err(|e| e.to_string())?;

            let mut decrypted_data = Vec::new();
            for shard in shards.iter().take(DATA_SHARDS) {
                if let Some(shard_data) = shard {
                    decrypted_data.extend_from_slice(shard_data);
                } else {
                    return Err(format!("Failed to reconstruct chunk {}", chunk_info.index));
                }
            }

            // Trim padding
            decrypted_data.truncate(chunk_info.size);

            // Verify that the decrypted data matches the original hash
            let calculated_hash = self.hash_chunk(&decrypted_data);
            if calculated_hash != chunk_info.hash {
                return Err(format!(
                    "Hash mismatch for chunk {}. Data may be corrupt.",
                    chunk_info.index
                ));
            }

            // Also verify the size
            if decrypted_data.len() != chunk_info.size {
                return Err(format!(
                    "Size mismatch for chunk {}. Expected {}, got {}.",
                    chunk_info.index, chunk_info.size, decrypted_data.len()
                ));
            }

            output_file.write_all(&decrypted_data).map_err(|e| e.to_string())?;
        }

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

    /// Generates a Merkle proof for a specific chunk.
    /// This would be called by a seeder node when a peer requests a chunk.
    pub fn generate_merkle_proof(
        &self,
        all_chunk_hashes_hex: &[String],
        chunk_index_to_prove: usize,
    ) -> Result<(Vec<usize>, Vec<String>, usize), String> {
        let all_chunk_hashes: Vec<[u8; 32]> = all_chunk_hashes_hex
            .iter()
            .map(|h| {
                hex::decode(h)
                    .map_err(|e| e.to_string())?
                    .try_into()
                    .map_err(|_| "Invalid chunk hash length".to_string())
            })
            .collect::<Result<Vec<_>, String>>()?;

        let merkle_tree = MerkleTree::<Sha256Hasher>::from_leaves(&all_chunk_hashes);
        let proof = merkle_tree.proof(&[chunk_index_to_prove]);

        let proof_indices = vec![chunk_index_to_prove];
        let proof_hashes_hex = proof.proof_hashes_hex();

        Ok((proof_indices, proof_hashes_hex, all_chunk_hashes.len()))
    }

    /// Verifies a downloaded chunk against the file's Merkle root using a proof.
    /// This is called by a downloader node to ensure chunk integrity.
    pub fn verify_chunk(
        &self,
        merkle_root_hex: &str,
        chunk_info: &ChunkInfo,
        chunk_data: &[u8],
        proof_indices: &[usize],
        proof_hashes_hex: &[String],
        total_leaves_count: usize,
    ) -> Result<bool, String> {
        // 1. Verify the chunk's own hash.
        let calculated_hash = Sha256Hasher::hash(chunk_data);
        if hex::encode(calculated_hash) != chunk_info.hash {
            return Ok(false); // The chunk data does not match its expected hash.
        }

        // 2. Decode hex strings to bytes for Merkle proof verification.
        let merkle_root: [u8; 32] = hex::decode(merkle_root_hex)
            .map_err(|e| e.to_string())?
            .try_into()
            .map_err(|_| "Invalid Merkle root length".to_string())?;

        let proof_hashes: Vec<[u8; 32]> = proof_hashes_hex
            .iter()
            .map(|h| {
                hex::decode(h)
                    .map_err(|e| e.to_string())?
                    .try_into()
                    .map_err(|_| "Invalid proof hash length".to_string())
            })
            .collect::<Result<Vec<_>, String>>()?;

        // 3. Construct a Merkle proof object and verify it against the root.
        let proof = rs_merkle::MerkleProof::<Sha256Hasher>::new(proof_hashes);
        Ok(proof.verify(merkle_root, proof_indices, &[calculated_hash], total_leaves_count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;
    use x25519_dalek::StaticSecret;

    #[test]
    fn test_chunk_encrypt_reassemble_decrypt() {
        // 1. Setup
        let dir = tempdir().unwrap();
        let storage_path = dir.path().to_path_buf();
        let manager = ChunkManager::new(storage_path.clone());

        let original_file_path = dir.path().join("original.txt");
        let reassembled_file_path = dir.path().join("reassembled.txt");
        let file_content = "This is a test file for erasure coding.".repeat(1000);
        fs::write(&original_file_path, &file_content).unwrap();

        let recipient_secret = StaticSecret::new(OsRng);
        let recipient_public = PublicKey::from(&recipient_secret);

        // 2. Chunk, encrypt, and apply erasure coding
        let manifest = manager.chunk_and_encrypt_file(&original_file_path, &recipient_public).unwrap();

        // 3. Reassemble, reconstruct from shards, and decrypt
        manager.reassemble_and_decrypt_file(
            &manifest.chunks,
            &reassembled_file_path,
            &manifest.encrypted_key_bundle,
            &recipient_secret,
        ).unwrap();

        // 4. Verify
        let reassembled_content = fs::read_to_string(&reassembled_file_path).unwrap();
        assert_eq!(file_content, reassembled_content);

        // 5. Cleanup is handled by tempdir dropping
    }
}
