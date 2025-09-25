use sha2::{Sha256, Digest};
use rs_merkle::{MerkleTree, Hasher};
use aes_gcm::{Aes256Gcm, Key, Nonce, KeyInit};
use aes_gcm::aead::{Aead, OsRng};
use rand::RngCore;
use std::fs::{File, self};
use std::io::{Read, Error, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::num::NonZeroUsize;
use lru::LruCache;
use once_cell::sync::Lazy;
use x25519_dalek::{EphemeralSecret, PublicKey};

// Import the new crypto functions and the bundle struct
use crate::crypto::{decrypt_aes_key, encrypt_aes_key, EncryptedAesKeyBundle};

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct ChunkInfo {
    pub index: u32,
    pub hash: String,
    pub size: usize,
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
    /// The encrypted  AES key bundle needed for decryption.
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

/// In-memory L1 cache for frequently accessed chunks.
/// We use a Mutex for thread-safe access and Lazy to ensure it's initialized only once.
static CHUNK_CACHE: Lazy<Mutex<LruCache<String, Vec<u8>>>> = Lazy::new(|| {
    // Cache up to 256 chunks (256 * 256KB = 64MB).
    let capacity = NonZeroUsize::new(256).unwrap();
    Mutex::new(LruCache::new(capacity))
});

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
            
            // The nonce is now prepended to the ciphertext by `encrypt_chunk`
            let encrypted_data_with_nonce = self.encrypt_chunk(chunk_data, &key)?;

            chunks_info.push(ChunkInfo {
                index,
                hash: chunk_hash_hex.clone(),
                size: bytes_read,
                encrypted_size: encrypted_data_with_nonce.len(),
            });

            self.save_chunk(&chunk_hash_hex, &encrypted_data_with_nonce).map_err(|e| e.to_string())?;
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
        // 1. Save to disk (L2 Cache)
        fs::create_dir_all(&self.storage_path)?;
        fs::write(self.storage_path.join(hash), data_with_nonce)?;

        // 2. Also put it in the in-memory cache (L1 Cache) for immediate access.
        if let Ok(mut cache) = CHUNK_CACHE.lock() {
            cache.put(hash.to_string(), data_with_nonce.to_vec());
        }

        Ok(())
    }

    pub fn read_chunk(&self, hash: &str) -> Result<Vec<u8>, Error> {
        // 1. Try to get the chunk from the in-memory L1 cache first.
        if let Ok(mut cache) = CHUNK_CACHE.lock() {
            if let Some(data) = cache.get(hash) {
                return Ok(data.clone()); // Cache hit! Return immediately.
            }
        }

        // 2. If not in memory (cache miss), read from disk (L2 cache).
        let data_from_disk = fs::read(self.storage_path.join(hash))?;

        // 3. Populate the L1 cache with the data we just read from disk.
        if let Ok(mut cache) = CHUNK_CACHE.lock() {
            cache.put(hash.to_string(), data_from_disk.clone());
        }

        Ok(data_from_disk)
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

    pub fn reassemble_and_decrypt_file(
        &self,
        chunks: &[ChunkInfo],
        output_path: &Path,
        encrypted_key_bundle: &EncryptedAesKeyBundle,
        recipient_secret_key: &EphemeralSecret,
    ) -> Result<(), String> {
        let key_bytes = decrypt_aes_key(encrypted_key_bundle, recipient_secret_key)?;
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);

        let mut output_file = File::create(output_path).map_err(|e| e.to_string())?;

        // Assuming chunks are ordered by index. If not, they should be sorted first.
        for chunk_info in chunks {
            let encrypted_data_with_nonce =
                self.read_chunk(&chunk_info.hash)
                    .map_err(|e| format!("Failed to read chunk {}: {}", chunk_info.index, e))?;

            let decrypted_data = self.decrypt_chunk(&encrypted_data_with_nonce, &key)?;

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
    ) -> Result<(Vec<usize>, Vec<String>), String> {
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

        
        let proof_indices = proof.proof_indices().to_vec();
        let proof_hashes_hex = proof.proof_hashes_hex();

        Ok((proof_indices, proof_hashes_hex))
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
        let proof = rs_merkle::MerkleProof::<Sha256Hasher>::new(proof_indices.to_vec(), proof_hashes);
        Ok(proof.verify(merkle_root, &[chunk_info.index as usize], &[calculated_hash]))
    }
}
