use sha2::{Sha256, Digest};
use aes_gcm::{Aes256Gcm, Key, Nonce, KeyInit};
use aes_gcm::aead::{Aead};
use std::fs::File;
use std::io::{Read, Error, Write};
use std::path::{Path, PathBuf};
use rand::{RngCore, rngs::OsRng};

pub struct ChunkInfo {
    pub index: u32,
    pub hash: String,
    pub nonce: Vec<u8>,
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

    pub fn chunk_file(&self, file_path: &Path) -> Result<(Vec<ChunkInfo>, Vec<u8>), Error> {
        let mut key_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut key_bytes);
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);

        let mut file = File::open(file_path)?;
        let mut chunks = Vec::new();
        let mut buffer = vec![0u8; self.chunk_size];
        let mut index = 0;

        loop {
            let bytes_read = file.read(&mut buffer)?;
            if bytes_read == 0 { break; }

            let chunk_data = &buffer[..bytes_read];
            let chunk_hash = self.hash_chunk(chunk_data);
            
            let (encrypted_data, nonce) = self.encrypt_chunk(chunk_data, &key).map_err(|e| Error::new(std::io::ErrorKind::Other, e.to_string()))?;

            chunks.push(ChunkInfo {
                index,
                hash: chunk_hash.clone(),
                nonce: nonce.clone(),
                size: bytes_read,
                encrypted_size: encrypted_data.len(),
            });

            self.save_chunk(&chunk_hash, &nonce, &encrypted_data)?;
            index += 1;
        }

        Ok((chunks, key_bytes.to_vec()))
    }

    fn encrypt_chunk(&self, data: &[u8], key: &Key<Aes256Gcm>) -> Result<(Vec<u8>, Vec<u8>), aes_gcm::Error> {
        let cipher = Aes256Gcm::new(key);
        
        let mut nonce_bytes = [0u8; 12]; // Using 12-byte IV
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher.encrypt(nonce, data)?;
        Ok((ciphertext, nonce_bytes.to_vec()))
    }

    fn hash_chunk(&self, data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }

    fn save_chunk(&self, hash: &str, nonce: &[u8], data: &[u8]) -> Result<(), Error> {
        let mut path = self.storage_path.clone();
        path.push(hash);
        let mut file = File::create(path)?;
        
        file.write_all(nonce)?;
        file.write_all(data)?;
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