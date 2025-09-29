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

            // Create data shards from the original chunk data.
            let mut shards: Vec<Vec<u8>> = chunk_data
                .chunks((chunk_data.len() + DATA_SHARDS - 1) / DATA_SHARDS)
                .map(|c| c.to_vec())
                .collect();

            // Calculate the size of the largest shard and pad all other shards to match.
            // This is crucial for the Reed-Solomon library to work correctly.
            let shard_len = shards.iter().map(|s| s.len()).max().unwrap_or(0);
            for shard in &mut shards {
                shard.resize(shard_len, 0);
            }

            // Add empty parity shards to be filled by the encoder.
            shards.resize(DATA_SHARDS + PARITY_SHARDS, vec![0; shard_len]);

            // Encode the data, creating the parity shards.
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

        let result: Result<(), String> = (|| {
            // Assuming chunks are ordered by index. If not, they should be sorted first.
            for chunk_info in chunks {
                // Gather all shards from storage. Missing shards will be `None`.
                let available_encrypted_shards: Vec<Option<Vec<u8>>> = chunk_info
                    .shards
                    .iter()
                    .map(|shard_hash| self.read_chunk(shard_hash).ok())
                    .collect();

                // Count available shards and fail fast if reconstruction is impossible.
                let available_shards = available_encrypted_shards.iter().filter(|s| s.is_some()).count();
                
                // Fix: We need at least DATA_SHARDS (10) out of total (14) shards to reconstruct
                if available_shards < DATA_SHARDS {
                    return Err(format!(
                        "Not enough shards to reconstruct chunk {}: found {}, need at least {}",
                        chunk_info.index, available_shards, DATA_SHARDS
                    ));
                }

                // Now, decrypt the available shards.
                let mut shards: Vec<Option<Vec<u8>>> = Vec::with_capacity(DATA_SHARDS + PARITY_SHARDS);
                for encrypted_shard_option in available_encrypted_shards {
                    if let Some(encrypted_shard) = encrypted_shard_option {
                        // If a shard is present but fails to decrypt, it's a critical error.
                        shards.push(Some(self.decrypt_chunk(&encrypted_shard, &key)?));
                    } else {
                        shards.push(None);
                    }
                }

                // Reconstruct all missing shards (data and parity).
                if let Err(e) = r.reconstruct(&mut shards) {
                    return Err(format!(
                        "Failed to reconstruct data for chunk {} from {} available shards: {:?}",
                        chunk_info.index, available_shards, e
                    ));
                }

                let mut decrypted_data = Vec::new();
                for shard in shards.iter().take(DATA_SHARDS) {
                    if let Some(shard_data) = shard {
                        decrypted_data.extend_from_slice(shard_data);
                    } else {
                        // This should not happen if reconstruction succeeded.
                        return Err(format!("Reconstruction of chunk {} failed unexpectedly: missing a data shard post-reconstruction.", chunk_info.index));
                    }
                }

                // Trim padding
                decrypted_data.truncate(chunk_info.size);

                // Verify that the decrypted data matches the original hash
                let calculated_hash_hex = hex::encode(Sha256Hasher::hash(&decrypted_data));
                if calculated_hash_hex != chunk_info.hash {
                    return Err(format!(
                        "Hash mismatch for chunk {}. Data may be corrupt. Expected: {}, Got: {}",
                        chunk_info.index, chunk_info.hash, calculated_hash_hex
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
        })();
        result
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
    use std::io::Seek;
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

        let recipient_secret = StaticSecret::random_from_rng(OsRng);
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

    #[test]
    fn test_reconstruction_with_missing_shards() {
        // 1. Setup
        let dir = tempdir().unwrap();
        let storage_path = dir.path().to_path_buf();
        let manager = ChunkManager::new(storage_path.clone());
 
        let original_file_path = dir.path().join("original_for_loss.txt");
        let reassembled_file_path = dir.path().join("reassembled_from_loss.txt");
        // Use enough content to create at least one full chunk
        let file_content = "This is a test file for erasure coding with simulated data loss.".repeat(5000);
        fs::write(&original_file_path, &file_content).unwrap();
 
        let recipient_secret = StaticSecret::random_from_rng(OsRng);
        let recipient_public = PublicKey::from(&recipient_secret);
 
        // 2. Chunk, encrypt, and apply erasure coding
        let manifest = manager.chunk_and_encrypt_file(&original_file_path, &recipient_public).unwrap();
 
        // 3. Simulate data loss by deleting some shards
        // We have 10 data + 4 parity shards. We can lose up to 4. Let's delete 3.
        let shards_to_delete = 3;
        if let Some(first_chunk_info) = manifest.chunks.first() {
            for i in 0..shards_to_delete {
                let shard_hash_to_delete = &first_chunk_info.shards[i];
                let shard_path = storage_path.join(shard_hash_to_delete);
                if shard_path.exists() {
                    fs::remove_file(shard_path).unwrap();
                }
            }
        }
 
        // 4. Attempt to reassemble the file from the incomplete set of shards
        manager.reassemble_and_decrypt_file(
            &manifest.chunks,
            &reassembled_file_path,
            &manifest.encrypted_key_bundle,
            &recipient_secret,
        ).unwrap();
 
        // 5. Verify that the file was reconstructed correctly despite the missing shards
        let reassembled_content = fs::read_to_string(&reassembled_file_path).unwrap();
        assert_eq!(file_content, reassembled_content);
    }

    #[test]
    fn test_reconstruction_fails_with_too_many_missing_shards() {
        // 1. Setup
        let dir = tempdir().unwrap();
        let storage_path = dir.path().to_path_buf();
        let manager = ChunkManager::new(storage_path.clone());

        let original_file_path = dir.path().join("original_for_failure.txt");
        let reassembled_file_path = dir.path().join("reassembled_from_failure.txt");
        let file_content = "This test should fail to reconstruct due to heavy data loss.".repeat(5000);
        fs::write(&original_file_path, &file_content).unwrap();

        let recipient_secret = StaticSecret::random_from_rng(OsRng);
        let recipient_public = PublicKey::from(&recipient_secret);

        // 2. Chunk, encrypt, and apply erasure coding
        let manifest = manager.chunk_and_encrypt_file(&original_file_path, &recipient_public).unwrap();
        assert!(!manifest.chunks.is_empty(), "Test file should produce at least one chunk");

        println!("Generated {} chunks", manifest.chunks.len());

        // 3. Simulate critical data loss by deleting too many shards
        // We have 10 data + 4 parity shards. We can lose up to 4. Let's delete 5.
        const SHARDS_TO_DELETE: usize = 5;
        if let Some(first_chunk_info) = manifest.chunks.first() {
            println!("First chunk has {} shards", first_chunk_info.shards.len());
            
            let mut actually_deleted = 0;
            for i in 0..SHARDS_TO_DELETE {
                let shard_hash_to_delete = &first_chunk_info.shards[i];
                let shard_path = storage_path.join(shard_hash_to_delete);
                if shard_path.exists() {
                    fs::remove_file(shard_path).unwrap();
                    actually_deleted += 1;
                    //println!("Deleted shard {}: {}", i, shard_hash_to_delete);
                } else {
                    println!("Shard {} doesn't exist: {}", i, shard_hash_to_delete);
                }
            }
            println!("Actually deleted {} shards", actually_deleted);
            
            // Count remaining shards
            let remaining_shards = first_chunk_info.shards.iter()
                .filter(|hash| storage_path.join(hash).exists())
                .count();
            println!("Remaining shards: {}", remaining_shards);
        }

        // CRITICAL FIX: Clear the L1 cache after deleting files
        // This ensures read_chunk() will actually fail for deleted shards
        {
            let mut cache = L1_CACHE.lock().unwrap();
            *cache = LruCache::new(L1_CACHE_CAPACITY);
            println!("Cleared L1 cache");
        }

        // 4. Attempt to reassemble the file. This should fail.
        let result = manager.reassemble_and_decrypt_file(
            &manifest.chunks,
            &reassembled_file_path,
            &manifest.encrypted_key_bundle,
            &recipient_secret,
        );

        // 5. Verify that the operation failed as expected.
        match result {
            Ok(_) => {
                panic!("Reconstruction succeeded when it should have failed. Check the debug output above.");
            },
            Err(error_message) => {
                // The error should indicate not enough shards were available.
                println!("Got expected error: {}", error_message);
                assert!(error_message.contains("Not enough shards to reconstruct chunk"));
            }
        }
    }
    #[test]
    fn test_merkle_proof_generation_and_verification() {
        // 1. Setup
        let dir = tempdir().unwrap();
        let storage_path = dir.path().to_path_buf();
        let manager = ChunkManager::new(storage_path.clone());

        let original_file_path = dir.path().join("original_for_merkle.txt");
        let file_content = "This is a test file for Merkle proof verification.".repeat(10000);
        fs::write(&original_file_path, &file_content).unwrap();

        let recipient_secret = StaticSecret::random_from_rng(OsRng);
        let recipient_public = PublicKey::from(&recipient_secret);

        // 2. Chunk the file to get the manifest, which contains the Merkle root and chunk hashes.
        let manifest = manager.chunk_and_encrypt_file(&original_file_path, &recipient_public).unwrap();
        assert!(manifest.chunks.len() > 1, "Test file should produce multiple chunks");

        // 3. Choose a chunk to prove and verify (e.g., the second chunk).
        let chunk_to_verify_index = 1;
        let chunk_info = &manifest.chunks[chunk_to_verify_index];
        let all_chunk_hashes: Vec<String> = manifest.chunks.iter().map(|c| c.hash.clone()).collect();

        // 4. Generate a Merkle proof for this chunk.
        let (proof_indices, proof_hashes, total_leaves) = manager.generate_merkle_proof(
            &all_chunk_hashes,
            chunk_to_verify_index
        ).unwrap();

        // 5. Simulate downloading the original chunk data.
        // For this test, we'll just read the original file to get the chunk data.
        let mut original_file = File::open(&original_file_path).unwrap();
        let mut buffer = vec![0; manager.chunk_size];
        original_file.seek(std::io::SeekFrom::Start((chunk_to_verify_index * manager.chunk_size) as u64)).unwrap();
        let bytes_read = original_file.read(&mut buffer).unwrap();
        let original_chunk_data = &buffer[..bytes_read];

        // 6. Verify the chunk using the proof.
        let is_valid = manager.verify_chunk(
            &manifest.merkle_root,
            chunk_info,
            original_chunk_data,
            &proof_indices,
            &proof_hashes,
            total_leaves,
        ).unwrap();

        assert!(is_valid, "Merkle proof verification should succeed for valid chunk data.");

        // 7. Negative test: Verify that tampered data fails verification.
        let mut tampered_data = original_chunk_data.to_vec();
        tampered_data[0] = tampered_data[0].wrapping_add(1); // Modify one byte
        let is_tampered_valid = manager.verify_chunk(&manifest.merkle_root, chunk_info, &tampered_data, &proof_indices, &proof_hashes, total_leaves).unwrap();
        assert!(!is_tampered_valid, "Merkle proof verification should fail for tampered data.");
    }
}
