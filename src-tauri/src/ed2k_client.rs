//! ed2k (eDonkey2000) Protocol Client
//!
//! This module implements a client for the ed2k protocol, used by eMule and similar P2P clients.
//! The ed2k protocol uses:
//! - Fixed chunk size: 9,728,000 bytes (9.28 MB)
//! - MD4 hash algorithm for file and chunk verification
//! - TCP connection to ed2k servers (default port 4661)

use md4::{Md4, Digest};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{debug, info};

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

    /// ED2K chunk hashes (MD4 hashes for each 9.28MB chunk)
    pub chunk_hashes: Vec<String>,
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

/// ED2K block size: 256 KB (used within 9.28MB chunks)
/// ED2K transfers files in 256KB blocks, which are grouped into 9.28MB chunks for hashing
pub const ED2K_BLOCK_SIZE: usize = 256 * 1024;

/// ed2k protocol opcodes
mod opcodes {
    // Server opcodes
    pub const OP_LOGINREQUEST: u8 = 0x01;
    pub const OP_SERVERMESSAGE: u8 = 0x38;
    pub const OP_SERVERLIST: u8 = 0x32;
    pub const OP_SERVERIDENT: u8 = 0x41;
    pub const OP_OFFERFILES: u8 = 0x15;
    pub const OP_GETSOURCES: u8 = 0x19;
    pub const OP_FOUNDSOURCES: u8 = 0x42;
    
    // Peer-to-peer opcodes
    pub const OP_REQUESTPARTS: u8 = 0x47;      // Request file parts from peer
    pub const OP_SENDINGPART: u8 = 0x46;       // Sending file part data
    pub const OP_FILEREQUEST: u8 = 0x58;       // Request file from peer
    pub const OP_FILEREQANSWER: u8 = 0x59;     // Answer to file request
    pub const OP_SETREQFILEID: u8 = 0x4F;      // Set requested file ID
    pub const OP_STARTUPLOADREQ: u8 = 0x54;    // Request to start upload
}

/// ED2K packet header structure
/// Format: [Protocol:1][Size:4][Opcode:1][Payload:Size-1]
#[derive(Debug, Clone)]
pub struct Ed2kPacketHeader {
    pub protocol: u8,      // Always 0xE3 for ED2K protocol
    pub size: u32,         // Size of payload + opcode (excludes protocol byte and size field)
    pub opcode: u8,        // Operation code
}

impl Ed2kPacketHeader {
    pub const HEADER_SIZE: usize = 6; // 1 byte protocol + 4 bytes size + 1 byte opcode
    pub const ED2K_PROTOCOL: u8 = 0xE3;
    
    /// Create a new packet header
    pub fn new(opcode: u8, payload_size: u32) -> Self {
        Self {
            protocol: Self::ED2K_PROTOCOL,
            size: payload_size + 1, // +1 for opcode
            opcode,
        }
    }
    
    /// Serialize header to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(Self::HEADER_SIZE);
        bytes.push(self.protocol);
        bytes.extend_from_slice(&self.size.to_le_bytes());
        bytes.push(self.opcode);
        bytes
    }
    
    /// Parse header from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Ed2kError> {
        if bytes.len() < Self::HEADER_SIZE {
            return Err(Ed2kError::ProtocolError("Incomplete packet header".to_string()));
        }
        
        let protocol = bytes[0];
        if protocol != Self::ED2K_PROTOCOL {
            return Err(Ed2kError::ProtocolError(format!(
                "Invalid protocol byte: 0x{:02x}, expected 0xE3",
                protocol
            )));
        }
        
        let size = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
        let opcode = bytes[5];
        
        Ok(Self { protocol, size, opcode })
    }
}

/// ED2K block request structure
/// Used to request a 256KB block from a peer
#[derive(Debug, Clone)]
pub struct Ed2kBlockRequest {
    pub file_hash: [u8; 16],   // MD4 hash of file
    pub start_offset: u64,     // Start byte offset
    pub end_offset: u64,       // End byte offset (exclusive)
}

impl Ed2kBlockRequest {
    /// Create a new block request
    pub fn new(file_hash: [u8; 16], start_offset: u64, end_offset: u64) -> Self {
        Self {
            file_hash,
            start_offset,
            end_offset,
        }
    }
    
    /// Serialize to bytes for OP_REQUESTPARTS packet
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(16 + 8 + 8);
        bytes.extend_from_slice(&self.file_hash);
        bytes.extend_from_slice(&self.start_offset.to_le_bytes());
        bytes.extend_from_slice(&self.end_offset.to_le_bytes());
        bytes
    }
    
    /// Parse from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Ed2kError> {
        if bytes.len() < 32 {
            return Err(Ed2kError::ProtocolError("Incomplete block request".to_string()));
        }
        
        let mut file_hash = [0u8; 16];
        file_hash.copy_from_slice(&bytes[0..16]);
        
        let start_offset = u64::from_le_bytes([
            bytes[16], bytes[17], bytes[18], bytes[19],
            bytes[20], bytes[21], bytes[22], bytes[23],
        ]);
        
        let end_offset = u64::from_le_bytes([
            bytes[24], bytes[25], bytes[26], bytes[27],
            bytes[28], bytes[29], bytes[30], bytes[31],
        ]);
        
        Ok(Self {
            file_hash,
            start_offset,
            end_offset,
        })
    }
}

/// ed2k client for downloading files
pub struct Ed2kClient {
    config: Ed2kConfig,
    connection: Option<TcpStream>,
    /// Files we're currently sharing (hash -> file info)
    shared_files: std::collections::HashMap<String, Ed2kFileInfo>,
    /// Our client ID assigned by server
    client_id: Option<u32>,
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
            shared_files: std::collections::HashMap::new(),
            client_id: None,
        }
    }

    /// Create a new ed2k client with custom configuration
    pub fn with_config(config: Ed2kConfig) -> Self {
        Self {
            config,
            connection: None,
            shared_files: std::collections::HashMap::new(),
            client_id: None,
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
        let computed_hash = Self::compute_md4_hash(data);
        computed_hash.eq_ignore_ascii_case(expected_hash)
    }

    /// Compute MD4 hash of data and return as hex string
    pub fn compute_md4_hash(data: &[u8]) -> String {
        let mut hasher = Md4::new();
        hasher.update(data);
        let result = hasher.finalize();
        format!("{:x}", result)
    }

    /// Compute ED2K file hash
    /// For files <= 9.28MB: returns MD4 hash of the entire file
    /// For files > 9.28MB: returns MD4 hash of the concatenated chunk hashes (root hash)
    pub async fn compute_file_hash<P: AsRef<Path>>(path: P) -> Result<String, Ed2kError> {
        let mut file = File::open(path).await?;
        let metadata = file.metadata().await?;
        let file_size = metadata.len() as usize;

        // Single chunk file
        if file_size <= ED2K_CHUNK_SIZE {
            let mut buffer = Vec::with_capacity(file_size);
            file.read_to_end(&mut buffer).await?;
            Ok(Self::compute_md4_hash(&buffer))
        } else {
            // Multi-chunk file: compute hash of chunk hashes
            let chunk_hashes = Self::compute_chunk_hashes_from_file(&mut file, file_size).await?;
            
            // Concatenate all chunk hash bytes
            let mut combined_hashes = Vec::new();
            for hash_str in chunk_hashes {
                let hash_bytes = hex::decode(&hash_str)?;
                combined_hashes.extend_from_slice(&hash_bytes);
            }
            
            Ok(Self::compute_md4_hash(&combined_hashes))
        }
    }

    /// Compute chunk hashes for a file
    /// Returns a vector of MD4 hash strings, one for each 9.28MB chunk
    pub async fn compute_chunk_hashes<P: AsRef<Path>>(path: P) -> Result<Vec<String>, Ed2kError> {
        let mut file = File::open(path).await?;
        let metadata = file.metadata().await?;
        let file_size = metadata.len() as usize;
        
        Self::compute_chunk_hashes_from_file(&mut file, file_size).await
    }

    /// Internal helper to compute chunk hashes from an open file
    async fn compute_chunk_hashes_from_file(file: &mut File, file_size: usize) -> Result<Vec<String>, Ed2kError> {
        let num_chunks = (file_size + ED2K_CHUNK_SIZE - 1) / ED2K_CHUNK_SIZE;
        let mut chunk_hashes = Vec::with_capacity(num_chunks);
        let mut buffer = vec![0u8; ED2K_CHUNK_SIZE];

        for _ in 0..num_chunks {
            let bytes_read = file.read(&mut buffer).await?;
            if bytes_read == 0 {
                break;
            }
            
            let chunk_hash = Self::compute_md4_hash(&buffer[..bytes_read]);
            chunk_hashes.push(chunk_hash);
        }

        Ok(chunk_hashes)
    }

    /// Split data into ED2K chunks (9.28MB each)
    /// Returns a vector of byte slices representing each chunk
    pub fn split_into_chunks(data: &[u8]) -> Vec<&[u8]> {
        let mut chunks = Vec::new();
        let mut offset = 0;

        while offset < data.len() {
            let end = std::cmp::min(offset + ED2K_CHUNK_SIZE, data.len());
            chunks.push(&data[offset..end]);
            offset = end;
        }

        chunks
    }

    /// Validate a complete file against ED2K hash
    /// Returns true if the computed hash matches the expected hash
    pub async fn validate_file<P: AsRef<Path>>(
        path: P,
        expected_hash: &str,
    ) -> Result<bool, Ed2kError> {
        let computed_hash = Self::compute_file_hash(path).await?;
        Ok(computed_hash.eq_ignore_ascii_case(expected_hash))
    }

    /// Validate a single chunk against its expected hash
    pub fn validate_chunk(chunk_data: &[u8], expected_hash: &str) -> bool {
        Self::verify_md4_hash(chunk_data, expected_hash)
    }

    /// Get the number of chunks needed for a file of given size
    pub fn get_chunk_count(file_size: u64) -> usize {
        ((file_size as usize) + ED2K_CHUNK_SIZE - 1) / ED2K_CHUNK_SIZE
    }

    /// Get the size of a specific chunk (last chunk may be smaller)
    pub fn get_chunk_size(chunk_index: usize, file_size: u64) -> usize {
        let total_chunks = Self::get_chunk_count(file_size);
        
        if chunk_index >= total_chunks {
            return 0;
        }
        
        if chunk_index == total_chunks - 1 {
            // Last chunk
            let remainder = (file_size as usize) % ED2K_CHUNK_SIZE;
            if remainder == 0 {
                ED2K_CHUNK_SIZE
            } else {
                remainder
            }
        } else {
            ED2K_CHUNK_SIZE
        }
    }

    /// Create ED2K file info from a local file
    /// Computes all necessary hashes and metadata
    pub async fn create_file_info<P: AsRef<Path>>(path: P) -> Result<Ed2kFileInfo, Ed2kError> {
        let path_ref = path.as_ref();
        let metadata = tokio::fs::metadata(path_ref).await?;
        let file_size = metadata.len();
        
        let file_name = path_ref
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string());
        
        let file_hash = Self::compute_file_hash(path_ref).await?;
        let chunk_hashes = if file_size > ED2K_CHUNK_SIZE as u64 {
            Self::compute_chunk_hashes(path_ref).await?
        } else {
            Vec::new()
        };
        
        Ok(Ed2kFileInfo {
            file_hash,
            file_size,
            file_name,
            sources: Vec::new(),
            chunk_hashes,
        })
    }

    /// Offer files to the ED2K server for sharing
    /// This announces to the server that we have these files available
    pub async fn offer_files(&mut self, files: Vec<Ed2kFileInfo>) -> Result<(), Ed2kError> {
        if self.connection.is_none() {
            return Err(Ed2kError::ConnectionError("Not connected to server".to_string()));
        }

        let conn = self.connection.as_mut().unwrap();

        // Build OP_OFFERFILES packet
        // Format: [opcode:1][file_count:4][files...]
        // Each file: [hash:16][client_id:4][port:2][tags...]
        let mut packet = Vec::new();
        packet.push(opcodes::OP_OFFERFILES);
        packet.extend_from_slice(&(files.len() as u32).to_le_bytes());

        for file in &files {
            // File hash (16 bytes MD4)
            let hash_bytes = hex::decode(&file.file_hash)
                .map_err(|e| Ed2kError::ProtocolError(format!("Invalid hash: {}", e)))?;
            if hash_bytes.len() != 16 {
                return Err(Ed2kError::ProtocolError("Hash must be 16 bytes".to_string()));
            }
            packet.extend_from_slice(&hash_bytes);

            // Client ID (4 bytes) - use our assigned ID or 0
            packet.extend_from_slice(&self.client_id.unwrap_or(0).to_le_bytes());

            // Port (2 bytes) - ED2K default port
            packet.extend_from_slice(&ED2K_DEFAULT_PORT.to_le_bytes());

            // Tag count (4 bytes) - we'll send 2 tags: filename and filesize
            packet.extend_from_slice(&2u32.to_le_bytes());

            // Tag 1: Filename (type 0x02 = string, id 0x01 = filename)
            if let Some(ref name) = file.file_name {
                packet.push(0x02); // String tag type
                packet.push(0x01); // Filename tag ID
                let name_bytes = name.as_bytes();
                packet.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
                packet.extend_from_slice(name_bytes);
            }

            // Tag 2: Filesize (type 0x03 = integer, id 0x02 = filesize)
            packet.push(0x03); // Integer tag type
            packet.push(0x02); // Filesize tag ID
            packet.extend_from_slice(&(file.file_size as u32).to_le_bytes());

            // Track locally
            self.shared_files.insert(file.file_hash.clone(), file.clone());
        }

        // Send packet
        conn.write_all(&packet).await?;

        Ok(())
    }

    /// Remove a file from sharing
    pub async fn remove_shared_file(&mut self, file_hash: &str) -> Result<(), Ed2kError> {
        // Remove from local tracking
        self.shared_files.remove(file_hash);

        // Re-send offer with remaining files to update server
        // (ED2K doesn't have explicit "unshare" - we just re-offer without the file)
        if self.connection.is_some() && !self.shared_files.is_empty() {
            let files: Vec<Ed2kFileInfo> = self.shared_files.values().cloned().collect();
            self.offer_files(files).await?;
        }

        Ok(())
    }

    /// Get list of files we're currently sharing
    pub fn get_shared_files(&self) -> Vec<Ed2kFileInfo> {
        self.shared_files.values().cloned().collect()
    }

    /// Get file information from ed2k network
    pub async fn get_file_info(&mut self, file_hash: &str) -> Result<Ed2kFileInfo, Ed2kError> {
        // Check if we have it locally first
        if let Some(info) = self.shared_files.get(file_hash) {
            return Ok(info.clone());
        }

        // Request sources to get file info
        let sources = self.get_sources(file_hash).await?;

        Ok(Ed2kFileInfo {
            file_hash: file_hash.to_string(),
            file_size: 0, // Unknown until we query a source
            file_name: None,
            sources,
            chunk_hashes: Vec::new(), // Will be populated from server metadata
        })
    }

    /// Get source list for a file from the server
    pub async fn get_sources(&mut self, file_hash: &str) -> Result<Vec<String>, Ed2kError> {
        if self.connection.is_none() {
            return Err(Ed2kError::ConnectionError("Not connected to server".to_string()));
        }

        let conn = self.connection.as_mut().unwrap();

        // Parse file hash
        let hash_bytes = hex::decode(file_hash)?;
        if hash_bytes.len() != 16 {
            return Err(Ed2kError::ProtocolError("Hash must be 16 bytes (MD4)".to_string()));
        }

        // Build OP_GETSOURCES packet
        let mut packet = Vec::new();
        packet.push(opcodes::OP_GETSOURCES);
        packet.extend_from_slice(&hash_bytes);

        // Send request
        conn.write_all(&packet).await?;

        // Read response (OP_FOUNDSOURCES)
        let mut header = [0u8; 5];
        let read_result = tokio::time::timeout(
            self.config.timeout,
            conn.read_exact(&mut header)
        ).await;

        match read_result {
            Ok(Ok(_)) => {
                if header[0] != opcodes::OP_FOUNDSOURCES {
                    return Err(Ed2kError::ProtocolError(
                        format!("Unexpected response opcode: 0x{:02x}", header[0])
                    ));
                }

                // Parse source count
                let source_count = u32::from_le_bytes([header[1], header[2], header[3], header[4]]) as usize;
                let mut sources = Vec::with_capacity(source_count);

                // Read source entries (6 bytes each: 4 bytes IP + 2 bytes port)
                for _ in 0..source_count {
                    let mut source_data = [0u8; 6];
                    conn.read_exact(&mut source_data).await?;

                    let ip = format!(
                        "{}.{}.{}.{}",
                        source_data[0], source_data[1], source_data[2], source_data[3]
                    );
                    let port = u16::from_le_bytes([source_data[4], source_data[5]]);

                    sources.push(format!("{}:{}", ip, port));
                }

                Ok(sources)
            }
            Ok(Err(e)) => Err(Ed2kError::IoError(e)),
            Err(_) => Err(Ed2kError::Timeout),
        }
    }

    /// Get server information
    pub async fn get_server_info(&mut self) -> Result<Ed2kServerInfo, Ed2kError> {
        // Server info is typically received after login
        // For now, return basic info
        Ok(Ed2kServerInfo {
            name: "ED2K Server".to_string(),
            description: Some("Connected ED2K server".to_string()),
            users: 0,
            files: 0,
        })
    }

    /// Search for files on ed2k network
    pub async fn search(&mut self, _query: &str) -> Result<Vec<Ed2kSearchResult>, Ed2kError> {
        // Search requires OP_SEARCHREQUEST - implement if needed
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

    // Peer-to-peer communication methods

    /// Connect to an ED2K peer for file transfer
    pub async fn connect_to_peer(addr: &str, timeout_secs: u64) -> Result<TcpStream, Ed2kError> {
        let stream = tokio::time::timeout(
            Duration::from_secs(timeout_secs),
            TcpStream::connect(addr)
        )
        .await
        .map_err(|_| Ed2kError::Timeout)?
        .map_err(|e| Ed2kError::ConnectionError(e.to_string()))?;

        info!("Connected to ED2K peer: {}", addr);
        Ok(stream)
    }

    /// Send an ED2K packet to a peer
    pub async fn send_packet(
        stream: &mut TcpStream,
        opcode: u8,
        payload: &[u8],
    ) -> Result<(), Ed2kError> {
        let header = Ed2kPacketHeader::new(opcode, payload.len() as u32);
        let header_bytes = header.to_bytes();
        
        // Write header
        stream.write_all(&header_bytes).await?;
        
        // Write payload
        if !payload.is_empty() {
            stream.write_all(payload).await?;
        }
        
        stream.flush().await?;
        
        debug!("Sent ED2K packet: opcode=0x{:02x}, size={}", opcode, payload.len());
        Ok(())
    }

    /// Receive an ED2K packet from a peer
    pub async fn receive_packet(
        stream: &mut TcpStream,
        timeout_secs: u64,
    ) -> Result<(u8, Vec<u8>), Ed2kError> {
        // Read header
        let mut header_bytes = [0u8; Ed2kPacketHeader::HEADER_SIZE];
        
        tokio::time::timeout(
            Duration::from_secs(timeout_secs),
            stream.read_exact(&mut header_bytes)
        )
        .await
        .map_err(|_| Ed2kError::Timeout)??;
        
        let header = Ed2kPacketHeader::from_bytes(&header_bytes)?;
        
        // Read payload (size includes opcode, so subtract 1)
        let payload_size = header.size.saturating_sub(1) as usize;
        let mut payload = vec![0u8; payload_size];
        
        if payload_size > 0 {
            tokio::time::timeout(
                Duration::from_secs(timeout_secs),
                stream.read_exact(&mut payload)
            )
            .await
            .map_err(|_| Ed2kError::Timeout)??;
        }
        
        debug!("Received ED2K packet: opcode=0x{:02x}, size={}", header.opcode, payload_size);
        
        Ok((header.opcode, payload))
    }

    /// Request a block from a peer
    pub async fn request_block_from_peer(
        stream: &mut TcpStream,
        file_hash: &str,
        start_offset: u64,
        end_offset: u64,
    ) -> Result<(), Ed2kError> {
        let hash_bytes = hex::decode(file_hash)?;
        if hash_bytes.len() != 16 {
            return Err(Ed2kError::ProtocolError("File hash must be 16 bytes".to_string()));
        }
        
        let mut hash_array = [0u8; 16];
        hash_array.copy_from_slice(&hash_bytes);
        
        let request = Ed2kBlockRequest::new(hash_array, start_offset, end_offset);
        let payload = request.to_bytes();
        
        Self::send_packet(stream, opcodes::OP_REQUESTPARTS, &payload).await?;
        
        info!(
            "Requested block: offset={}-{}, size={}",
            start_offset,
            end_offset,
            end_offset - start_offset
        );
        
        Ok(())
    }

    /// Download a block from a peer
    /// Returns the block data
    pub async fn download_block_from_peer(
        stream: &mut TcpStream,
        file_hash: &str,
        start_offset: u64,
        block_size: usize,
    ) -> Result<Vec<u8>, Ed2kError> {
        let end_offset = start_offset + block_size as u64;
        
        // Send request
        Self::request_block_from_peer(stream, file_hash, start_offset, end_offset).await?;
        
        // Receive response
        let (opcode, payload) = Self::receive_packet(stream, 60).await?;
        
        if opcode != opcodes::OP_SENDINGPART {
            return Err(Ed2kError::ProtocolError(format!(
                "Expected OP_SENDINGPART (0x{:02x}), got 0x{:02x}",
                opcodes::OP_SENDINGPART,
                opcode
            )));
        }
        
        // Parse payload: [file_hash:16][start:8][end:8][data:...]
        if payload.len() < 32 {
            return Err(Ed2kError::ProtocolError("Invalid SENDINGPART payload".to_string()));
        }
        
        let data = payload[32..].to_vec();
        
        info!("Downloaded block: size={} bytes", data.len());
        
        Ok(data)
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
    fn test_compute_md4_hash() {
        let data = b"hello world";
        let hash = Ed2kClient::compute_md4_hash(data);
        assert_eq!(hash, "aa010fbc1d14c795d86ef98c95479d17");
    }

    #[test]
    fn test_compute_md4_hash_empty() {
        let data = b"";
        let hash = Ed2kClient::compute_md4_hash(data);
        // MD4 hash of empty string
        assert_eq!(hash, "31d6cfe0d16ae931b73c59d7e0c089c0");
    }

    #[test]
    fn test_split_into_chunks_small_file() {
        let data = vec![0u8; 1000];
        let chunks = Ed2kClient::split_into_chunks(&data);
        
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), 1000);
    }

    #[test]
    fn test_split_into_chunks_exact_chunk_size() {
        let data = vec![0u8; ED2K_CHUNK_SIZE];
        let chunks = Ed2kClient::split_into_chunks(&data);
        
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), ED2K_CHUNK_SIZE);
    }

    #[test]
    fn test_split_into_chunks_multiple() {
        let data = vec![0u8; ED2K_CHUNK_SIZE * 2 + 1000];
        let chunks = Ed2kClient::split_into_chunks(&data);
        
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), ED2K_CHUNK_SIZE);
        assert_eq!(chunks[1].len(), ED2K_CHUNK_SIZE);
        assert_eq!(chunks[2].len(), 1000);
    }

    #[test]
    fn test_get_chunk_count_small() {
        assert_eq!(Ed2kClient::get_chunk_count(1000), 1);
    }

    #[test]
    fn test_get_chunk_count_exact() {
        assert_eq!(Ed2kClient::get_chunk_count(ED2K_CHUNK_SIZE as u64), 1);
    }

    #[test]
    fn test_get_chunk_count_multiple() {
        assert_eq!(Ed2kClient::get_chunk_count((ED2K_CHUNK_SIZE * 2 + 1000) as u64), 3);
    }

    #[test]
    fn test_get_chunk_size_first() {
        let file_size = (ED2K_CHUNK_SIZE * 2 + 1000) as u64;
        assert_eq!(Ed2kClient::get_chunk_size(0, file_size), ED2K_CHUNK_SIZE);
    }

    #[test]
    fn test_get_chunk_size_middle() {
        let file_size = (ED2K_CHUNK_SIZE * 3) as u64;
        assert_eq!(Ed2kClient::get_chunk_size(1, file_size), ED2K_CHUNK_SIZE);
    }

    #[test]
    fn test_get_chunk_size_last_partial() {
        let file_size = (ED2K_CHUNK_SIZE * 2 + 1000) as u64;
        assert_eq!(Ed2kClient::get_chunk_size(2, file_size), 1000);
    }

    #[test]
    fn test_get_chunk_size_last_full() {
        let file_size = (ED2K_CHUNK_SIZE * 2) as u64;
        assert_eq!(Ed2kClient::get_chunk_size(1, file_size), ED2K_CHUNK_SIZE);
    }

    #[test]
    fn test_get_chunk_size_out_of_bounds() {
        let file_size = ED2K_CHUNK_SIZE as u64;
        assert_eq!(Ed2kClient::get_chunk_size(10, file_size), 0);
    }

    #[test]
    fn test_validate_chunk_valid() {
        let data = b"test data";
        let hash = Ed2kClient::compute_md4_hash(data);
        assert!(Ed2kClient::validate_chunk(data, &hash));
    }

    #[test]
    fn test_validate_chunk_invalid() {
        let data = b"test data";
        let wrong_hash = "0000000000000000000000000000000";
        assert!(!Ed2kClient::validate_chunk(data, wrong_hash));
    }

    #[tokio::test]
    async fn test_compute_file_hash_small() {
        use tokio::io::AsyncWriteExt;
        
        // Create a temporary file
        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join("ed2k_test_small.dat");
        
        let mut file = tokio::fs::File::create(&file_path).await.unwrap();
        file.write_all(b"hello world").await.unwrap();
        file.flush().await.unwrap();
        drop(file);
        
        let hash = Ed2kClient::compute_file_hash(&file_path).await.unwrap();
        assert_eq!(hash, "aa010fbc1d14c795d86ef98c95479d17");
        
        // Cleanup
        tokio::fs::remove_file(file_path).await.ok();
    }

    #[tokio::test]
    async fn test_validate_file() {
        use tokio::io::AsyncWriteExt;
        
        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join("ed2k_test_validate.dat");
        
        let mut file = tokio::fs::File::create(&file_path).await.unwrap();
        file.write_all(b"test").await.unwrap();
        file.flush().await.unwrap();
        drop(file);
        
        let correct_hash = "db346d691d7acc4dc2625db19f9e3f52";
        let is_valid = Ed2kClient::validate_file(&file_path, correct_hash).await.unwrap();
        assert!(is_valid);
        
        let wrong_hash = "0000000000000000000000000000000";
        let is_invalid = Ed2kClient::validate_file(&file_path, wrong_hash).await.unwrap();
        assert!(!is_invalid);
        
        // Cleanup
        tokio::fs::remove_file(file_path).await.ok();
    }

    #[tokio::test]
    async fn test_create_file_info() {
        use tokio::io::AsyncWriteExt;
        
        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join("ed2k_test_info.dat");
        
        let mut file = tokio::fs::File::create(&file_path).await.unwrap();
        let test_data = b"test file content";
        file.write_all(test_data).await.unwrap();
        file.flush().await.unwrap();
        drop(file);
        
        let file_info = Ed2kClient::create_file_info(&file_path).await.unwrap();
        
        assert_eq!(file_info.file_size, test_data.len() as u64);
        assert_eq!(file_info.file_name, Some("ed2k_test_info.dat".to_string()));
        assert!(!file_info.file_hash.is_empty());
        assert_eq!(file_info.chunk_hashes.len(), 0); // Small file, no chunks
        
        // Cleanup
        tokio::fs::remove_file(file_path).await.ok();
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

    #[test]
    fn test_ed2k_block_size_constant() {
        assert_eq!(ED2K_BLOCK_SIZE, 256 * 1024);
    }

    #[test]
    fn test_packet_header_new() {
        let header = Ed2kPacketHeader::new(0x46, 1000);
        
        assert_eq!(header.protocol, Ed2kPacketHeader::ED2K_PROTOCOL);
        assert_eq!(header.size, 1001); // payload + opcode
        assert_eq!(header.opcode, 0x46);
    }

    #[test]
    fn test_packet_header_serialization() {
        let header = Ed2kPacketHeader::new(0x47, 256);
        let bytes = header.to_bytes();
        
        assert_eq!(bytes.len(), Ed2kPacketHeader::HEADER_SIZE);
        assert_eq!(bytes[0], 0xE3); // Protocol byte
        assert_eq!(bytes[5], 0x47); // Opcode
        
        // Size should be 257 (256 + 1 for opcode)
        let size = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
        assert_eq!(size, 257);
    }

    #[test]
    fn test_packet_header_deserialization() {
        let original = Ed2kPacketHeader::new(0x58, 512);
        let bytes = original.to_bytes();
        let parsed = Ed2kPacketHeader::from_bytes(&bytes).unwrap();
        
        assert_eq!(parsed.protocol, original.protocol);
        assert_eq!(parsed.size, original.size);
        assert_eq!(parsed.opcode, original.opcode);
    }

    #[test]
    fn test_packet_header_invalid_protocol() {
        let mut bytes = vec![0xFF, 0x00, 0x00, 0x00, 0x00, 0x47]; // Wrong protocol byte
        let result = Ed2kPacketHeader::from_bytes(&bytes);
        
        assert!(result.is_err());
    }

    #[test]
    fn test_packet_header_incomplete() {
        let bytes = vec![0xE3, 0x00, 0x00]; // Incomplete header
        let result = Ed2kPacketHeader::from_bytes(&bytes);
        
        assert!(result.is_err());
    }

    #[test]
    fn test_block_request_new() {
        let hash = [0x01; 16];
        let request = Ed2kBlockRequest::new(hash, 0, 262144);
        
        assert_eq!(request.file_hash, hash);
        assert_eq!(request.start_offset, 0);
        assert_eq!(request.end_offset, 262144);
    }

    #[test]
    fn test_block_request_serialization() {
        let hash = [0xAB; 16];
        let request = Ed2kBlockRequest::new(hash, 1000, 2000);
        let bytes = request.to_bytes();
        
        assert_eq!(bytes.len(), 32); // 16 + 8 + 8
        assert_eq!(&bytes[0..16], &hash);
    }

    #[test]
    fn test_block_request_deserialization() {
        let hash = [0xCD; 16];
        let original = Ed2kBlockRequest::new(hash, 512, 1024);
        let bytes = original.to_bytes();
        let parsed = Ed2kBlockRequest::from_bytes(&bytes).unwrap();
        
        assert_eq!(parsed.file_hash, original.file_hash);
        assert_eq!(parsed.start_offset, original.start_offset);
        assert_eq!(parsed.end_offset, original.end_offset);
    }

    #[test]
    fn test_block_request_incomplete() {
        let bytes = vec![0x00; 20]; // Too short
        let result = Ed2kBlockRequest::from_bytes(&bytes);
        
        assert!(result.is_err());
    }

    #[test]
    fn test_block_size_alignment() {
        // Verify ED2K_BLOCK_SIZE is 256KB
        assert_eq!(ED2K_BLOCK_SIZE, 262144);
        
        // Verify relationship: multiple blocks make up a chunk
        let blocks_per_chunk = ED2K_CHUNK_SIZE / ED2K_BLOCK_SIZE;
        assert_eq!(blocks_per_chunk, 37); // 9.28MB / 256KB = 37.109...
    }
}
