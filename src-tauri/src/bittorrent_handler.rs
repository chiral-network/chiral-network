use crate::protocols::ProtocolHandler;
use async_trait::async_trait;
use std::path::Path;
use tracing::{info, warn, error, instrument};

/// BitTorrent protocol handler implementing the ProtocolHandler trait.
/// This handler manages BitTorrent downloads and seeding operations.
pub struct BitTorrentHandler {
    download_directory: std::path::PathBuf,
}

impl BitTorrentHandler {
    /// Creates a new BitTorrentHandler with the specified download directory.
    pub fn new(download_directory: std::path::PathBuf) -> Self {
        info!("Initializing BitTorrentHandler with download directory: {:?}", download_directory);
        Self {
            download_directory,
        }
    }

    /// Checks if a string is a valid magnet link.
    fn is_magnet_link(identifier: &str) -> bool {
        identifier.starts_with("magnet:?xt=urn:btih:")
    }

    /// Checks if a string is a path to a torrent file.
    fn is_torrent_file(identifier: &str) -> bool {
        identifier.ends_with(".torrent") && Path::new(identifier).exists()
    }

    /// Extracts the info hash from a magnet link.
    fn extract_info_hash(magnet_link: &str) -> Option<String> {
        if let Some(start_pos) = magnet_link.find("xt=urn:btih:") {
            let start_idx = start_pos + "xt=urn:btih:".len();
            let remaining = &magnet_link[start_idx..];
            
            // Find the end of the info hash (either end of string or next parameter)
            let end_idx = remaining.find('&').unwrap_or(remaining.len());
            let info_hash = &remaining[..end_idx];
            
            // Validate that it's a valid hex string (40 characters for SHA-1)
            if info_hash.len() == 40 && info_hash.chars().all(|c| c.is_ascii_hexdigit()) {
                Some(info_hash.to_lowercase())
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Creates a magnet link from a file path.
    /// This is a simplified implementation that would need a real torrent library.
    async fn create_magnet_link(&self, file_path: &str) -> Result<String, String> {
        use sha1::{Sha1, Digest};
        use std::fs;

        let path = Path::new(file_path);
        if !path.exists() {
            return Err(format!("File does not exist: {}", file_path));
        }

        // Read file content to generate a pseudo info hash
        // In a real implementation, this would create a proper torrent info hash
        let file_content = fs::read(file_path)
            .map_err(|e| format!("Failed to read file: {}", e))?;
        
        let mut hasher = Sha1::new();
        hasher.update(&file_content);
        hasher.update(file_path.as_bytes()); // Include filename for uniqueness
        let info_hash = format!("{:x}", hasher.finalize());

        let file_name = path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown");

        // Create a basic magnet link
        let magnet_link = format!(
            "magnet:?xt=urn:btih:{}&dn={}",
            info_hash,
            urlencoding::encode(file_name)
        );

        Ok(magnet_link)
    }
}

#[async_trait]
impl ProtocolHandler for BitTorrentHandler {
    fn name(&self) -> &'static str {
        "bittorrent"
    }

    fn supports(&self, identifier: &str) -> bool {
        Self::is_magnet_link(identifier) || Self::is_torrent_file(identifier)
    }

    #[instrument(skip(self), fields(protocol = "bittorrent"))]
    async fn download(&self, identifier: &str) -> Result<(), String> {
        info!("Starting BitTorrent download for: {}", identifier);

        if Self::is_magnet_link(identifier) {
            let info_hash = Self::extract_info_hash(identifier)
                .ok_or_else(|| "Invalid magnet link format".to_string())?;
            
            info!("Extracted info hash: {}", info_hash);
            
            // TODO: Implement actual BitTorrent download using a library like `torrent_rs` or similar
            // For now, this is a placeholder implementation
            
            // Simulate download process
            info!("Simulating BitTorrent download for info hash: {}", info_hash);
            
            // Create download directory if it doesn't exist
            if !self.download_directory.exists() {
                std::fs::create_dir_all(&self.download_directory)
                    .map_err(|e| format!("Failed to create download directory: {}", e))?;
            }
            
            // In a real implementation, this would:
            // 1. Parse the magnet link completely
            // 2. Connect to trackers or DHT
            // 3. Find peers
            // 4. Download pieces
            // 5. Verify piece hashes
            // 6. Assemble the complete file
            
            info!("BitTorrent download completed (simulated) for: {}", identifier);
            Ok(())
            
        } else if Self::is_torrent_file(identifier) {
            info!("Processing torrent file: {}", identifier);
            
            // TODO: Implement torrent file parsing and download
            // This would involve:
            // 1. Parse the .torrent file (bencode format)
            // 2. Extract tracker information and file metadata
            // 3. Follow similar process as magnet link download
            
            info!("Torrent file download completed (simulated) for: {}", identifier);
            Ok(())
            
        } else {
            Err(format!("Unsupported identifier format: {}", identifier))
        }
    }

    #[instrument(skip(self), fields(protocol = "bittorrent"))]
    async fn seed(&self, file_path: &str) -> Result<String, String> {
        info!("Starting to seed file: {}", file_path);

        let path = Path::new(file_path);
        if !path.exists() {
            return Err(format!("File does not exist: {}", file_path));
        }

        // Generate magnet link for the file
        let magnet_link = self.create_magnet_link(file_path).await?;
        
        // TODO: Implement actual seeding logic
        // This would involve:
        // 1. Creating a .torrent file with proper metadata
        // 2. Announcing to trackers or DHT
        // 3. Listening for peer connections
        // 4. Serving file pieces to requesting peers
        
        info!("Seeding started (simulated) for file: {} with magnet link: {}", file_path, magnet_link);
        
        Ok(magnet_link)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn test_is_magnet_link() {
        assert!(BitTorrentHandler::is_magnet_link("magnet:?xt=urn:btih:1234567890abcdef1234567890abcdef12345678"));
        assert!(!BitTorrentHandler::is_magnet_link("http://example.com/file.torrent"));
        assert!(!BitTorrentHandler::is_magnet_link("not_a_magnet_link"));
    }

    #[test]
    fn test_extract_info_hash() {
        let magnet = "magnet:?xt=urn:btih:1234567890abcdef1234567890abcdef12345678&dn=example";
        let hash = BitTorrentHandler::extract_info_hash(magnet);
        assert_eq!(hash, Some("1234567890abcdef1234567890abcdef12345678".to_string()));
        
        let invalid_magnet = "magnet:?xt=urn:btih:invalid_hash";
        let invalid_hash = BitTorrentHandler::extract_info_hash(invalid_magnet);
        assert_eq!(invalid_hash, None);
    }

    #[tokio::test]
    async fn test_supports() {
        let temp_dir = tempdir().unwrap();
        let handler = BitTorrentHandler::new(temp_dir.path().to_path_buf());
        
        assert!(handler.supports("magnet:?xt=urn:btih:1234567890abcdef1234567890abcdef12345678"));
        assert!(!handler.supports("http://example.com/file.zip"));
        assert!(!handler.supports("ftp://example.com/file.zip"));
    }

    #[tokio::test]
    async fn test_create_magnet_link() {
        let temp_dir = tempdir().unwrap();
        let handler = BitTorrentHandler::new(temp_dir.path().to_path_buf());
        
        // Create a temporary file
        let file_path = temp_dir.path().join("test_file.txt");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "Hello, BitTorrent!").unwrap();
        
        let magnet_link = handler.create_magnet_link(file_path.to_str().unwrap()).await.unwrap();
        assert!(magnet_link.starts_with("magnet:?xt=urn:btih:"));
        assert!(magnet_link.contains("&dn=test_file.txt"));
    }

    #[tokio::test]
    async fn test_seed_nonexistent_file() {
        let temp_dir = tempdir().unwrap();
        let handler = BitTorrentHandler::new(temp_dir.path().to_path_buf());
        
        let result = handler.seed("/nonexistent/file.txt").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("File does not exist"));
    }
}