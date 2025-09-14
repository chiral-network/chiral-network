use sha2::{Sha256, Digest};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use std::fs::File;
use std::io::{Read, Error, Write};
use std::path::{Path, PathBuf};

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

    pub fn chunk_file(&self, file_path: &Path) -> Result<Vec<ChunkInfo>, Error> {
        let mut file = File::open(file_path)?;
        let mut chunks = Vec::new();
        let mut buffer = vec![0u8; self.chunk_size];
        let mut index = 0;

        loop {
            let bytes_read = file.read(&mut buffer)?;
            if bytes_read == 0 { break; }

            let chunk_data = &buffer[..bytes_read];
            let chunk_hash = self.hash_chunk(chunk_data);
            let encrypted = self.encrypt_chunk(chunk_data).map_err(|e| Error::new(std::io::ErrorKind::Other, e.to_string()))?;

            chunks.push(ChunkInfo {
                index,
                hash: chunk_hash.clone(),
                size: bytes_read,
                encrypted_size: encrypted.len(),
            });

            self.save_chunk(&chunk_hash, &encrypted)?;
            index += 1;
        }

        Ok(chunks)
    }

    fn encrypt_chunk(&self, data: &[u8]) -> Result<Vec<u8>, aes_gcm::Error> {
        let key = Key::<Aes256Gcm>::from_slice(b"encryption_key_32_bytes_long!!!!");
        let cipher = Aes256Gcm::new(key);
        let nonce = Nonce::from_slice(b"unique_nonce"); // Nonce should be unique for each encryption

        cipher.encrypt(nonce, data)
    }

    fn hash_chunk(&self, data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }

    fn save_chunk(&self, hash: &str, data: &[u8]) -> Result<(), Error> {
        let mut path = self.storage_path.clone();
        path.push(hash);
        let mut file = File::create(path)?;
        file.write_all(data)?;
        Ok(())
    }

    pub fn hash_file(&self, file_path: &Path) -> Result<String, Error> {
        let mut file = File::open(file_path)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0; 1024 * 1024]; // 1MB buffer

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