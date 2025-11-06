//! ed2k (eDonkey2000) Protocol Client
//!
//! This module implements a client for the ed2k protocol, used by eMule and similar P2P clients.
//! The ed2k protocol uses:
//! - Fixed chunk size: 9,728,000 bytes (9.28 MB)
//! - MD4 hash algorithm for file and chunk verification
//! - TCP connection to ed2k servers (default port 4661)

use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// ed2k chunk size: 9.28 MB (9,728,000 bytes)
pub const ED2K_CHUNK_SIZE: usize = 9_728_000;

/// Default ed2k server port
pub const ED2K_DEFAULT_PORT: u16 = 4661;

/// ed2k client configuration
#[derive(Debug, Clone)]
pub struct Ed2kConfig {
    /// ed2k server URL (e.g., "ed2k://|server|176.103.48.36|4661|/")
    pub server_url: String,

    /// Connection timeout
    pub timeout: Duration,

    /// Client ID (generated or assigned by server)
    pub client_id: Option<String>,
}

impl Default for Ed2kConfig {
    fn default() -> Self {
        Self {
            server_url: String::new(),
            timeout: Duration::from_secs(30),
            client_id: None,
        }
    }
}

/// Information about an ed2k file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ed2kFileInfo {
    /// File hash (MD4)
    pub file_hash: String,

    /// File size in bytes
    pub file_size: u64,

    /// File name
    pub file_name: Option<String>,

    /// Available sources (IP:Port)
    pub sources: Vec<String>,
}

/// ed2k server information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ed2kServerInfo {
    /// Server name
    pub name: String,

    /// Server description
    pub description: Option<String>,

    /// Number of users
    pub users: u32,

    /// Number of files
    pub files: u32,
}

/// ed2k search result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ed2kSearchResult {
    /// File hash
    pub file_hash: String,

    /// File name
    pub file_name: String,

    /// File size
    pub file_size: u64,

    /// Number of sources
    pub source_count: u32,
}

/// ed2k client for downloading files
pub struct Ed2kClient {
    config: Ed2kConfig,
    connection: Option<TcpStream>,
}

/// ed2k protocol errors
#[derive(Debug, thiserror::Error)]
pub enum Ed2kError {
    #[error("Connection error: {0}")]
    ConnectionError(String),

    #[error("Protocol error: {0}")]
    ProtocolError(String),

    #[error("Hash verification failed")]
    HashMismatch,

    #[error("Timeout")]
    Timeout,

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Invalid hex string: {0}")]
    HexError(#[from] hex::FromHexError),
}

impl Ed2kClient {
    /// Create a new ed2k client
    pub fn new(server_url: String) -> Self {
        Self {
            config: Ed2kConfig {
                server_url,
                ..Default::default()
            },
            connection: None,
        }
    }

    /// Create a new ed2k client with custom configuration
    pub fn with_config(config: Ed2kConfig) -> Self {
        Self {
            config,
            connection: None,
        }
    }

    /// Parse ed2k server URL
    /// Format: ed2k://|server|IP|PORT|/
    pub fn parse_server_url(url: &str) -> Result<(String, u16), Ed2kError> {
        if !url.starts_with("ed2k://") {
            return Err(Ed2kError::ProtocolError("Invalid ed2k URL - must start with ed2k://".to_string()));
        }

        let parts: Vec<&str> = url.trim_start_matches("ed2k://")
            .trim_end_matches('/')
            .split('|')
            .filter(|s| !s.is_empty()) // Filter out empty parts
            .collect();

        if parts.len() < 3 || parts[0] != "server" {
            return Err(Ed2kError::ProtocolError("Invalid server URL format - expected ed2k://|server|IP|PORT|/".to_string()));
        }

        let ip = parts[1].to_string();
        let port = parts[2].parse::<u16>()
            .map_err(|_| Ed2kError::ProtocolError("Invalid port number".to_string()))?;

        Ok((ip, port))
    }

    /// Connect to ed2k server
    pub async fn connect(&mut self) -> Result<(), Ed2kError> {
        // Parse server URL
        let (ip, port) = Self::parse_server_url(&self.config.server_url)?;

        // Connect with timeout
        let addr = format!("{}:{}", ip, port);
        let stream = tokio::time::timeout(
            self.config.timeout,
            TcpStream::connect(&addr)
        )
        .await
        .map_err(|_| Ed2kError::Timeout)?
        .map_err(|e| Ed2kError::ConnectionError(e.to_string()))?;

        self.connection = Some(stream);

        // In a real implementation, we would send a login packet here
        // For now, we just establish the connection

        Ok(())
    }

    /// Download a specific chunk (9.28 MB)
    pub async fn download_chunk(
        &mut self,
        file_hash: &str,
        chunk_index: u32,
        expected_chunk_hash: &str,
    ) -> Result<Vec<u8>, Ed2kError> {
        // Ensure connected
        if self.connection.is_none() {
            return Err(Ed2kError::ConnectionError("Not connected to server".to_string()));
        }

        let conn = self.connection.as_mut().unwrap();

        // 1. Validate and decode file hash
        let file_hash_bytes = hex::decode(file_hash)?;

        if file_hash_bytes.len() != 16 {
            return Err(Ed2kError::ProtocolError("File hash must be 16 bytes (MD4)".to_string()));
        }

        // 2. Build request packet
        // ed2k protocol format (simplified):
        // - Opcode: 0x58 (OP_REQUESTPARTS)
        // - File hash: 16 bytes (MD4)
        // - Chunk index: 4 bytes (little-endian)
        let mut request = Vec::new();
        request.push(0x58); // OP_REQUESTPARTS opcode
        request.extend_from_slice(&file_hash_bytes);
        request.extend_from_slice(&chunk_index.to_le_bytes());

        // 3. Send request
        conn.write_all(&request).await?;

        // 4. Receive chunk data (9.28 MB)
        let mut chunk_data = Vec::with_capacity(ED2K_CHUNK_SIZE);

        // Read with timeout
        let read_result = tokio::time::timeout(
            self.config.timeout,
            async {
                let mut buffer = vec![0u8; 8192]; // 8KB buffer
                let mut total_read = 0;

                while total_read < ED2K_CHUNK_SIZE {
                    let bytes_read = conn.read(&mut buffer).await?;
                    if bytes_read == 0 {
                        break; // EOF or connection closed
                    }

                    let end = std::cmp::min(bytes_read, ED2K_CHUNK_SIZE - total_read);
                    chunk_data.extend_from_slice(&buffer[..end]);
                    total_read += end;

                    if total_read >= ED2K_CHUNK_SIZE {
                        break;
                    }
                }

                Ok::<Vec<u8>, std::io::Error>(chunk_data)
            }
        )
        .await
        .map_err(|_| Ed2kError::Timeout)??;

        // 5. Verify chunk hash (MD4)
        if !Self::verify_md4_hash(&read_result, expected_chunk_hash) {
            return Err(Ed2kError::HashMismatch);
        }

        Ok(read_result)
    }

    /// Verify MD4 hash of data
    pub fn verify_md4_hash(data: &[u8], expected_hash: &str) -> bool {
        use md4::Digest;

        let mut hasher = md4::Md4::new();
        hasher.update(data);
        let result = hasher.finalize();
        let computed_hash = format!("{:x}", result);

        computed_hash.eq_ignore_ascii_case(expected_hash)
    }

    /// Get file information from ed2k network
    pub async fn get_file_info(&mut self, file_hash: &str) -> Result<Ed2kFileInfo, Ed2kError> {
        // Placeholder implementation
        // In a real implementation, this would query the server
        Ok(Ed2kFileInfo {
            file_hash: file_hash.to_string(),
            file_size: 0,
            file_name: None,
            sources: Vec::new(),
        })
    }

    /// Get source list for a file
    pub async fn get_sources(&mut self, file_hash: &str) -> Result<Vec<String>, Ed2kError> {
        // Placeholder implementation
        // In a real implementation, this would request sources from the server
        Ok(Vec::new())
    }

    /// Get server information
    pub async fn get_server_info(&mut self) -> Result<Ed2kServerInfo, Ed2kError> {
        // Placeholder implementation
        Ok(Ed2kServerInfo {
            name: "ed2k Server".to_string(),
            description: Some("Test server".to_string()),
            users: 0,
            files: 0,
        })
    }

    /// Search for files on ed2k network
    pub async fn search(&mut self, query: &str) -> Result<Vec<Ed2kSearchResult>, Ed2kError> {
        // Placeholder implementation
        Ok(Vec::new())
    }

    /// Disconnect from ed2k server
    pub async fn disconnect(&mut self) -> Result<(), Ed2kError> {
        if let Some(mut conn) = self.connection.take() {
            conn.shutdown().await?;
        }
        Ok(())
    }

    /// Check if currently connected
    pub fn is_connected(&self) -> bool {
        self.connection.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_md4_hash_verification() {
        // Known MD4 hash for "hello world"
        let data = b"hello world";
        let expected_hash = "aa010fbc1d14c795d86ef98c95479d17";

        assert!(Ed2kClient::verify_md4_hash(data, expected_hash));
    }

    #[test]
    fn test_md4_hash_mismatch() {
        let data = b"hello world";
        let wrong_hash = "0000000000000000000000000000000";

        assert!(!Ed2kClient::verify_md4_hash(data, wrong_hash));
    }

    #[test]
    fn test_md4_hash_case_insensitive() {
        let data = b"test";
        let hash_upper = "DB346D691D7ACC4DC2625DB19F9E3F52";
        let hash_lower = "db346d691d7acc4dc2625db19f9e3f52";

        assert!(Ed2kClient::verify_md4_hash(data, hash_upper));
        assert!(Ed2kClient::verify_md4_hash(data, hash_lower));
    }

    #[test]
    fn test_parse_valid_server_url() {
        let url = "ed2k://|server|176.103.48.36|4661|/";
        let result = Ed2kClient::parse_server_url(url);

        assert!(result.is_ok());
        let (ip, port) = result.unwrap();
        assert_eq!(ip, "176.103.48.36");
        assert_eq!(port, 4661);
    }

    #[test]
    fn test_parse_invalid_protocol() {
        let url = "http://example.com";
        let result = Ed2kClient::parse_server_url(url);

        assert!(result.is_err());
    }

    #[test]
    fn test_parse_missing_parts() {
        let url = "ed2k://|server|/";
        let result = Ed2kClient::parse_server_url(url);

        assert!(result.is_err());
    }

    #[test]
    fn test_parse_invalid_port() {
        let url = "ed2k://|server|176.103.48.36|invalid|/";
        let result = Ed2kClient::parse_server_url(url);

        assert!(result.is_err());
    }

    #[test]
    fn test_create_ed2k_client() {
        let client = Ed2kClient::new("ed2k://|server|127.0.0.1|4661|/".to_string());
        assert!(!client.is_connected());
    }

    #[test]
    fn test_ed2k_chunk_size_constant() {
        assert_eq!(ED2K_CHUNK_SIZE, 9_728_000);
    }
}
