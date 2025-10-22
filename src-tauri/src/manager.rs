use aes_gcm::aead::{Aead, AeadCore, OsRng};
use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce};
use rand::RngCore;
use rs_merkle::{Hasher, MerkleTree};
use sha2::Digest;
use std::fs::{self, File};
use std::io::{Error, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use x25519_dalek::PublicKey;

// Import the new encryption functions and the bundle struct
use crate::encryption::{decrypt_aes_key, encrypt_aes_key, DiffieHellman, EncryptedAesKeyBundle};

use lazy_static::lazy_static;
use std::collections::HashMap;

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

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct ChunkInfo {
    pub index: u32,
    pub hash: String,
    pub size: usize,
    pub encrypted_hash: String,
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
    /// The encrypted AES key bundle needed for decryption (None for unencrypted files).
    pub encrypted_key_bundle: Option<EncryptedAesKeyBundle>,
}

/// A simple Sha256 hasher implementation for the Merkle tree.
#[derive(Clone)]
pub struct Sha256Hasher;

impl Hasher for Sha256Hasher {
    type Hash = [u8; 32];

    fn hash(data: &[u8]) -> [u8; 32] {
        let mut hasher = sha2::Sha256::default();
        hasher.update(data);
        hasher.finalize().into()
    }
}

pub struct ChunkManager {
    chunk_size: usize,
    storage_path: PathBuf,
}

/// The result of a canonical, one-time encryption of a file.
pub struct CanonicalEncryptionResult {
    pub manifest: FileManifest,
    pub canonical_aes_key: [u8; 32],
}

impl ChunkManager {
    pub fn new(storage_path: PathBuf) -> Self {
        ChunkManager {
            chunk_size: 256 * 1024, // 256KB
            storage_path,
        }
    }

    pub fn chunk_and_encrypt_file(
        &self,
        file_path: &Path,
        recipient_public_key: &PublicKey,
    ) -> Result<FileManifest, String> {
        let canonical_result = self.chunk_and_encrypt_file_canonical(file_path)?;
        let mut manifest = canonical_result.manifest;
        let canonical_aes_key = canonical_result.canonical_aes_key;

        let encrypted_bundle =
            encrypt_aes_key(&canonical_aes_key, recipient_public_key)?;

        manifest.encrypted_key_bundle = Some(encrypted_bundle);

        Ok(manifest)
    }

    /// Encrypts a file once with a new, canonical AES key.
    /// This function is the first step in publishing a new encrypted file. It returns the manifest
    /// (which is public and key-agnostic) and the raw AES key, which the caller MUST store securely.
    pub fn chunk_and_encrypt_file_canonical(
        &self,
        file_path: &Path,
    ) -> Result<CanonicalEncryptionResult, String> {
        // 1. Generate a new, single-use canonical AES key for the entire file.
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
            if bytes_read == 0 {
                break;
            }

            let chunk_data = &buffer[..bytes_read];
            // Hash the original, unencrypted chunk for the Merkle root.
            let chunk_hash_bytes = Sha256Hasher::hash(chunk_data);
            chunk_hashes.push(chunk_hash_bytes);
            let chunk_hash_hex = hex::encode(chunk_hash_bytes);

            // Encrypt the chunk with the canonical key.
            let encrypted_chunk_with_nonce = self.encrypt_chunk(chunk_data, &key)?;
            let encrypted_chunk_hash = Self::hash_data(&encrypted_chunk_with_nonce);
            self.save_chunk(&encrypted_chunk_hash, &encrypted_chunk_with_nonce)
                .map_err(|e| e.to_string())?;

            chunks_info.push(ChunkInfo {
                index,
                hash: chunk_hash_hex.clone(),
                size: bytes_read,
                encrypted_hash: encrypted_chunk_hash,
                encrypted_size: encrypted_chunk_with_nonce.len(),
            });

            index += 1;
        }

        // Build the Merkle tree from the original chunk hashes.
        let merkle_tree = MerkleTree::<Sha256Hasher>::from_leaves(&chunk_hashes);
        let merkle_root = merkle_tree.root().ok_or("Failed to compute Merkle root")?;

        // Create a key-agnostic manifest. The key bundle will be added later for each recipient.
        let manifest = FileManifest {
            merkle_root: hex::encode(merkle_root),
            chunks: chunks_info,
            encrypted_key_bundle: None,
        };

        // Return the manifest AND the raw AES key for secure storage by the caller.
        Ok(CanonicalEncryptionResult {
            manifest,
            canonical_aes_key: key_bytes,
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

    fn hash_data(data: &[u8]) -> String {
        let mut hasher = sha2::Sha256::default();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }

    // This function now saves the combined [nonce][ciphertext] blob
    fn save_chunk(&self, hash: &str, data_with_nonce: &[u8]) -> Result<(), Error> {
        fs::create_dir_all(&self.storage_path)?;
        let chunk_path = self.storage_path.join(hash);
        // --- Deduplication: Only write if the chunk does not already exist ---
        if chunk_path.exists() {
            // Already present, skip writing
            // Prime the L1 cache anyway
            let mut cache = L1_CACHE.lock().unwrap();
            cache.put(hash.to_string(), data_with_nonce.to_vec());
            return Ok(());
        }
        fs::write(&chunk_path, data_with_nonce)?;
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

    fn decrypt_chunk(
        &self,
        data_with_nonce: &[u8],
        key: &Key<Aes256Gcm>,
    ) -> Result<Vec<u8>, String> {
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
        encrypted_key_bundle: &Option<EncryptedAesKeyBundle>,
        recipient_secret_key: S,
    ) -> Result<(), String> {
        let key_bytes = match encrypted_key_bundle {
            Some(bundle) => decrypt_aes_key(bundle, recipient_secret_key)?,
            None => return Err("No encryption key bundle provided for encrypted file".to_string()),
        };
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);

        let mut output_file = File::create(output_path).map_err(|e| e.to_string())?;

        // Assuming chunks are ordered by index. If not, they should be sorted first.
        let result: Result<(), String> = (|| {
            for chunk_info in chunks {
                // Read the encrypted chunk from storage
                let encrypted_chunk = self.read_chunk(&chunk_info.encrypted_hash).map_err(|e| {
                    format!("Failed to read encrypted chunk {}: {}", chunk_info.index, e)
                })?;

                // Decrypt the chunk
                let decrypted_data = self.decrypt_chunk(&encrypted_chunk, &key)?;

                // Trim padding to original size
                let mut decrypted_data = decrypted_data;
                decrypted_data.truncate(chunk_info.size);

                // Verify that the decrypted data matches the original hash
                let calculated_hash_hex = hex::encode(Sha256Hasher::hash(&decrypted_data));
                if calculated_hash_hex != chunk_info.hash {
                    return Err(format!(
                        "Hash mismatch for chunk {}. Data may be corrupt. Expected: {}, Got: {}",
                        chunk_info.index, chunk_info.hash, calculated_hash_hex
                    ));
                }

                output_file
                    .write_all(&decrypted_data)
                    .map_err(|e| e.to_string())?;
            }
            Ok(())
        })();
        result
    }

    /// Decrypts and reassembles chunks into an in-memory byte vector.
    pub fn reassemble_and_decrypt_data<S: DiffieHellman>(
        &self,
        chunks: &[ChunkInfo],
        encrypted_key_bundle: &Option<EncryptedAesKeyBundle>,
        recipient_secret_key: S,
    ) -> Result<Vec<u8>, String> {
        let key_bytes = match encrypted_key_bundle {
            Some(bundle) => decrypt_aes_key(bundle, recipient_secret_key)?,
            None => return Err("No encryption key bundle provided for encrypted file".to_string()),
        };
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);

        let mut file_data = Vec::new();

        for chunk_info in chunks {
            // Read the encrypted chunk from storage
            let encrypted_chunk = self.read_chunk(&chunk_info.encrypted_hash).map_err(|e| {
                format!("Failed to read encrypted chunk {}: {}", chunk_info.index, e)
            })?;

            // Decrypt the chunk
            let mut decrypted_data = self.decrypt_chunk(&encrypted_chunk, &key)?;
            decrypted_data.truncate(chunk_info.size);

            // Verify that the decrypted data matches the original hash
            let calculated_hash_hex = hex::encode(Sha256Hasher::hash(&decrypted_data));
            if calculated_hash_hex != chunk_info.hash {
                return Err(format!(
                    "Hash mismatch for chunk {}. Data may be corrupt. Expected: {}, Got: {}",
                    chunk_info.index, chunk_info.hash, calculated_hash_hex
                ));
            }

            file_data.extend_from_slice(&decrypted_data);
        }

        Ok(file_data)
    }

    pub fn hash_file(&self, file_path: &Path) -> Result<String, Error> {
        let mut file = File::open(file_path)?;
        let mut hasher = sha2::Sha256::default();
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
        // Convert proof hashes to Vec<String> (hex)
        let proof_hashes_hex: Vec<String> = proof
            .proof_hashes()
            .iter()
            .map(|h| hex::encode(h))
            .collect();

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
        Ok(proof.verify(
            merkle_root,
            proof_indices,
            &[calculated_hash],
            total_leaves_count,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Seek;
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
        let file_content = "This is a test file for chunking and encryption.".repeat(1000);
        fs::write(&original_file_path, &file_content).unwrap();

        let recipient_secret = StaticSecret::random_from_rng(OsRng);
        let recipient_public = PublicKey::from(&recipient_secret);

        // 2. Chunk and encrypt the file
        let manifest = manager
            .chunk_and_encrypt_file(&original_file_path, &recipient_public)
            .unwrap();

        // 3. Reassemble and decrypt
        manager
            .reassemble_and_decrypt_file(
                &manifest.chunks,
                &reassembled_file_path,
                &manifest.encrypted_key_bundle,
                &recipient_secret,
            )
            .unwrap();

        // 4. Verify
        let reassembled_content = fs::read_to_string(&reassembled_file_path).unwrap();
        assert_eq!(file_content, reassembled_content);

        // 5. Cleanup is handled by tempdir dropping
    }

    #[test]
    fn test_merkle_tree_proof_and_verification() {
        // 1. Create some mock chunk data and their hashes (leaves)
        let leaves_data = vec![
            "chunk 0 data",
            "chunk 1 data",
            "chunk 2 data",
            "chunk 3 data",
            "chunk 4 data",
        ];
        let leaves: Vec<[u8; 32]> = leaves_data
            .iter()
            .map(|d| Sha256Hasher::hash(d.as_bytes()))
            .collect();

        // 2. Build the Merkle Tree
        let merkle_tree = MerkleTree::<Sha256Hasher>::from_leaves(&leaves);
        let merkle_root = merkle_tree.root().expect("Could not get Merkle root");

        // 3. Generate a proof for a specific leaf (e.g., index 2)
        let index_to_prove = 2;
        let leaf_to_prove = leaves[index_to_prove];
        let proof = merkle_tree.proof(&[index_to_prove]);
        let proof_hashes = proof.proof_hashes();

        // 4. Verify the proof
        let is_valid = proof.verify(
            merkle_root,
            &[index_to_prove],
            &[leaf_to_prove],
            leaves.len(),
        );
        assert!(
            is_valid,
            "Merkle proof verification should succeed for the correct leaf."
        );

        // 5. Test an invalid case: try to verify the proof with a different leaf
        let wrong_leaf_index = 3;
        let wrong_leaf = leaves[wrong_leaf_index];
        let is_invalid = proof.verify(
            merkle_root,
            &[index_to_prove], // Proof is for index 2
            &[wrong_leaf],     // But we provide data from index 3
            leaves.len(),
        );
        assert!(
            !is_invalid,
            "Merkle proof verification should fail for an incorrect leaf."
        );
    }

    #[test]
    fn test_reconstruction_with_missing_chunks() {
        // 1. Setup
        let dir = tempdir().unwrap();
        let storage_path = dir.path().to_path_buf();
        let manager = ChunkManager::new(storage_path.clone());

        let original_file_path = dir.path().join("original_for_loss.txt");
        let reassembled_file_path = dir.path().join("reassembled_from_loss.txt");
        // Use enough content to create at least one full chunk
        let file_content =
            "This is a test file for chunking with simulated data loss.".repeat(5000);
        fs::write(&original_file_path, &file_content).unwrap();

        let recipient_secret = StaticSecret::random_from_rng(OsRng);
        let recipient_public = PublicKey::from(&recipient_secret);

        // 2. Chunk and encrypt the file
        let manifest = manager
            .chunk_and_encrypt_file(&original_file_path, &recipient_public)
            .unwrap();

        // 3. Simulate data loss by deleting some chunks
        // Delete the first chunk
        if let Some(first_chunk_info) = manifest.chunks.first() {
            let chunk_path = storage_path.join(&first_chunk_info.encrypted_hash);
            if chunk_path.exists() {
                fs::remove_file(chunk_path).unwrap();
            }
        }

        // 4. Attempt to reassemble the file. This should fail since we can't reconstruct missing chunks
        let result = manager.reassemble_and_decrypt_file(
            &manifest.chunks,
            &reassembled_file_path,
            &manifest.encrypted_key_bundle,
            &recipient_secret,
        );

        // 5. Verify that the operation failed as expected.
        match result {
            Ok(_) => {
                panic!("Reconstruction succeeded when it should have failed due to missing chunk.");
            }
            Err(error_message) => {
                // The error should indicate the chunk could not be read
                assert!(error_message.contains("Failed to read encrypted chunk"));
            }
        }
    }

    #[test]
    fn test_reconstruction_fails_with_missing_chunk() {
        // 1. Setup
        let dir = tempdir().unwrap();
        let storage_path = dir.path().to_path_buf();
        let manager = ChunkManager::new(storage_path.clone());

        let original_file_path = dir.path().join("original_for_failure.txt");
        let reassembled_file_path = dir.path().join("reassembled_from_failure.txt");
        let file_content =
            "This test should fail to reconstruct due to missing chunk.".repeat(5000);
        fs::write(&original_file_path, &file_content).unwrap();

        let recipient_secret = StaticSecret::random_from_rng(OsRng);
        let recipient_public = PublicKey::from(&recipient_secret);

        // 2. Chunk and encrypt the file
        let manifest = manager
            .chunk_and_encrypt_file(&original_file_path, &recipient_public)
            .unwrap();
        assert!(
            !manifest.chunks.is_empty(),
            "Test file should produce at least one chunk"
        );

        // 3. Simulate chunk loss by deleting the first chunk
        if let Some(first_chunk_info) = manifest.chunks.first() {
            let chunk_path = storage_path.join(&first_chunk_info.encrypted_hash);
            if chunk_path.exists() {
                fs::remove_file(chunk_path).unwrap();
            }
        }

        // CRITICAL FIX: Clear the L1 cache after deleting files
        // This ensures read_chunk() will actually fail for deleted chunks
        {
            let mut cache = L1_CACHE.lock().unwrap();
            *cache = LruCache::new(L1_CACHE_CAPACITY);
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
            }
            Err(error_message) => {
                // The error should indicate the chunk could not be read
                assert!(error_message.contains("Failed to read encrypted chunk"));
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
        let manifest = manager
            .chunk_and_encrypt_file(&original_file_path, &recipient_public)
            .unwrap();
        assert!(
            manifest.chunks.len() > 1,
            "Test file should produce multiple chunks"
        );

        // 3. Choose a chunk to prove and verify (e.g., the second chunk).
        let chunk_to_verify_index = 1;
        let chunk_info = &manifest.chunks[chunk_to_verify_index];
        let all_chunk_hashes: Vec<String> =
            manifest.chunks.iter().map(|c| c.hash.clone()).collect();

        // 4. Generate a Merkle proof for this chunk.
        let (proof_indices, proof_hashes, total_leaves) = manager
            .generate_merkle_proof(&all_chunk_hashes, chunk_to_verify_index)
            .unwrap();

        // 5. Simulate downloading the original chunk data.
        // For this test, we'll just read the original file to get the chunk data.
        let mut original_file = File::open(&original_file_path).unwrap();
        let mut buffer = vec![0; manager.chunk_size];
        original_file
            .seek(std::io::SeekFrom::Start(
                (chunk_to_verify_index * manager.chunk_size) as u64,
            ))
            .unwrap();
        let bytes_read = original_file.read(&mut buffer).unwrap();
        let original_chunk_data = &buffer[..bytes_read];

        // 6. Verify the chunk using the proof.
        let is_valid = manager
            .verify_chunk(
                &manifest.merkle_root,
                chunk_info,
                original_chunk_data,
                &proof_indices,
                &proof_hashes,
                total_leaves,
            )
            .unwrap();

        assert!(
            is_valid,
            "Merkle proof verification should succeed for valid chunk data."
        );

        // 7. Negative test: Verify that tampered data fails verification.
        let mut tampered_data = original_chunk_data.to_vec();
        tampered_data[0] = tampered_data[0].wrapping_add(1); // Modify one byte
        let is_tampered_valid = manager
            .verify_chunk(
                &manifest.merkle_root,
                chunk_info,
                &tampered_data,
                &proof_indices,
                &proof_hashes,
                total_leaves,
            )
            .unwrap();
        assert!(
            !is_tampered_valid,
            "Merkle proof verification should fail for tampered data."
        );
    }
}
