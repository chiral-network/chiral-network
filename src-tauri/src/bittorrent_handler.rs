use crate::protocols::ProtocolHandler;
use async_trait::async_trait;
use std::path::Path;
use tracing::{info, error, instrument};
use sha1::{Sha1, Digest}; // Import Sha1 and Digest for hashing

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
        use std::fs; // `sha1::{Sha1, Digest}` is already imported at the top of the file now.

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

    /// Verifies the SHA-1 integrity of an assembled file.
    /// This is a helper function for post-download verification.
    fn verify_assembled_file_integrity(file_path: &Path, expected_sha1_hash: &str) -> Result<(), String> {
        use std::fs;
        info!("Verifying integrity of assembled file: {:?}", file_path);

        if !file_path.exists() {
            return Err(format!("File does not exist for verification: {:?}", file_path));
        }

        let file_content = fs::read(file_path)
            .map_err(|e| format!("Failed to read file for integrity check: {}", e))?;

        let mut hasher = Sha1::new();
        hasher.update(&file_content);
        let actual_sha1_hash = format!("{:x}", hasher.finalize());

        if actual_sha1_hash == expected_sha1_hash.to_lowercase() {
            info!("File integrity verified successfully. Actual hash: {}", actual_sha1_hash);
            Ok(())
        } else {
            error!(
                "File integrity verification failed. Expected: {}, Actual: {}",
                expected_sha1_hash, actual_sha1_hash
            );
            Err(format!("File integrity mismatch. Expected: {}, Actual: {}", expected_sha1_hash, actual_sha1_hash))
        }
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
            //    - Extract the info_hash and piece hashes from the .torrent file.
            // 3. Follow similar process as magnet link download
            //    - For each downloaded piece:
            //      - Calculate its SHA-1 hash.
            //      - Compare with the expected piece hash from the torrent metadata.
            //      - If hashes don't match, request the piece again or from another peer.
            
            info!("Torrent file download completed (simulated) for: {}", identifier);

            // Simulate creating a dummy file for verification
            // In a real scenario, the info_hash would be extracted from the .torrent file.
            let simulated_info_hash = "0123456789abcdef0123456789abcdef01234567".to_string(); // Placeholder
            let dummy_file_name = format!("{}.bin", simulated_info_hash);
            let dummy_file_path = self.download_directory.join(&dummy_file_name);
            std::fs::write(&dummy_file_path, b"simulated file content from torrent file")
                .map_err(|e| format!("Failed to write dummy file: {}", e))?;

            // Verify the integrity of the simulated assembled file
            Self::verify_assembled_file_integrity(&dummy_file_path, &simulated_info_hash)?;

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
        // 4. Serving file pieces to requesting peers.
        //    - When serving a piece, ensure its integrity (e.g., re-hash and compare before sending).
        
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
    use tokio;

    // Helper function to create a test file with known content
    fn create_test_file(dir: &std::path::Path, name: &str, content: &str) -> std::path::PathBuf {
        let file_path = dir.join(name);
        let mut file = File::create(&file_path).unwrap();
        write!(file, "{}", content).unwrap();
        file_path
    }

    // Unit Tests for Protocol Detection
    #[test]
    fn test_is_magnet_link_valid_formats() {
        // Valid magnet links
        assert!(BitTorrentHandler::is_magnet_link("magnet:?xt=urn:btih:1234567890abcdef1234567890abcdef12345678"));
        assert!(BitTorrentHandler::is_magnet_link("magnet:?xt=urn:btih:ABCDEF1234567890ABCDEF1234567890ABCDEF12"));
        assert!(BitTorrentHandler::is_magnet_link("magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=test"));
        
        // Invalid formats
        assert!(!BitTorrentHandler::is_magnet_link("magnet:?xt=urn:btmh:1234567890abcdef1234567890abcdef12345678"));
        assert!(!BitTorrentHandler::is_magnet_link("http://example.com/file.torrent"));
        assert!(!BitTorrentHandler::is_magnet_link("not_a_magnet_link"));
        assert!(!BitTorrentHandler::is_magnet_link(""));
        assert!(!BitTorrentHandler::is_magnet_link("magnet:"));
    }

    #[test]
    fn test_is_torrent_file() {
        let temp_dir = tempdir().unwrap();
        
        // Create a real torrent file
        let torrent_path = create_test_file(temp_dir.path(), "test.torrent", "torrent content");
        assert!(BitTorrentHandler::is_torrent_file(torrent_path.to_str().unwrap()));
        
        // Non-existent torrent file
        assert!(!BitTorrentHandler::is_torrent_file("/nonexistent/file.torrent"));
        
        // Wrong extension
        let txt_path = create_test_file(temp_dir.path(), "test.txt", "text content");
        assert!(!BitTorrentHandler::is_torrent_file(txt_path.to_str().unwrap()));
    }

    // Unit Tests for Info Hash Extraction
    #[test]
    fn test_extract_info_hash_various_formats() {
        // Standard magnet link
        let magnet1 = "magnet:?xt=urn:btih:1234567890abcdef1234567890abcdef12345678&dn=example";
        assert_eq!(
            BitTorrentHandler::extract_info_hash(magnet1),
            Some("1234567890abcdef1234567890abcdef12345678".to_string())
        );
        
        // Magnet link without additional parameters
        let magnet2 = "magnet:?xt=urn:btih:abcdef1234567890abcdef1234567890abcdef12";
        assert_eq!(
            BitTorrentHandler::extract_info_hash(magnet2),
            Some("abcdef1234567890abcdef1234567890abcdef12".to_string())
        );
        
        // Magnet link with multiple parameters
        let magnet3 = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=file&tr=http://tracker.example.com";
        assert_eq!(
            BitTorrentHandler::extract_info_hash(magnet3),
            Some("0123456789abcdef0123456789abcdef01234567".to_string())
        );
        
        // Invalid cases
        assert_eq!(BitTorrentHandler::extract_info_hash("magnet:?xt=urn:btih:invalid_hash"), None);
        assert_eq!(BitTorrentHandler::extract_info_hash("magnet:?xt=urn:btih:123"), None); // Too short
        assert_eq!(BitTorrentHandler::extract_info_hash("magnet:?xt=urn:btih:1234567890abcdef1234567890abcdef123456789"), None); // Too long
        assert_eq!(BitTorrentHandler::extract_info_hash("not_a_magnet"), None);
    }

    // Unit Tests for Protocol Handler Implementation
    #[tokio::test]
    async fn test_name() {
        let temp_dir = tempdir().unwrap();
        let handler = BitTorrentHandler::new(temp_dir.path().to_path_buf());
        assert_eq!(handler.name(), "bittorrent");
    }

    #[tokio::test]
    async fn test_supports_comprehensive() {
        let temp_dir = tempdir().unwrap();
        let handler = BitTorrentHandler::new(temp_dir.path().to_path_buf());
        
        // Should support magnet links
        assert!(handler.supports("magnet:?xt=urn:btih:1234567890abcdef1234567890abcdef12345678"));
        assert!(handler.supports("magnet:?xt=urn:btih:abcdef1234567890abcdef1234567890abcdef12&dn=test"));
        
        // Should support existing torrent files
        let torrent_path = create_test_file(temp_dir.path(), "test.torrent", "torrent content");
        assert!(handler.supports(torrent_path.to_str().unwrap()));
        
        // Should not support other protocols
        assert!(!handler.supports("http://example.com/file.zip"));
        assert!(!handler.supports("ftp://example.com/file.zip"));
        assert!(!handler.supports("file:///path/to/file.txt"));
        assert!(!handler.supports("/nonexistent/file.torrent"));
        assert!(!handler.supports(""));
    }

    // Unit Tests for Magnet Link Creation
    #[tokio::test]
    async fn test_create_magnet_link_valid_file() {
        let temp_dir = tempdir().unwrap();
        let handler = BitTorrentHandler::new(temp_dir.path().to_path_buf());
        
        let file_content = "Hello, BitTorrent World!";
        let file_path = create_test_file(temp_dir.path(), "test_file.txt", file_content);
        
        let magnet_link = handler.create_magnet_link(file_path.to_str().unwrap()).await.unwrap();
        
        // Verify magnet link format
        assert!(magnet_link.starts_with("magnet:?xt=urn:btih:"));
        assert!(magnet_link.contains("&dn=test_file.txt"));
        
        // Verify info hash is 40 characters (SHA-1)
        let hash_start = magnet_link.find("urn:btih:").unwrap() + "urn:btih:".len();
        let hash_end = magnet_link.find("&").unwrap();
        let hash = &magnet_link[hash_start..hash_end];
        assert_eq!(hash.len(), 40);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[tokio::test]
    async fn test_create_magnet_link_nonexistent_file() {
        let temp_dir = tempdir().unwrap();
        let handler = BitTorrentHandler::new(temp_dir.path().to_path_buf());
        
        let result = handler.create_magnet_link("/nonexistent/file.txt").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("File does not exist"));
    }

    #[tokio::test]
    async fn test_create_magnet_link_deterministic() {
        let temp_dir = tempdir().unwrap();
        let handler = BitTorrentHandler::new(temp_dir.path().to_path_buf());
        
        let file_content = "Deterministic test content";
        let file_path = create_test_file(temp_dir.path(), "deterministic.txt", file_content);
        
        // Generate magnet link twice
        let magnet1 = handler.create_magnet_link(file_path.to_str().unwrap()).await.unwrap();
        let magnet2 = handler.create_magnet_link(file_path.to_str().unwrap()).await.unwrap();
        
        // Should be identical
        assert_eq!(magnet1, magnet2);
    }

    // Unit Tests for verify_assembled_file_integrity
    #[test]
    fn test_verify_assembled_file_integrity_success() {
        let temp_dir = tempdir().unwrap();
        let file_content = b"This is a test file for integrity verification.";
        let file_path = create_test_file(temp_dir.path(), "verified_file.txt", std::str::from_utf8(file_content).unwrap());

        let mut hasher = Sha1::new();
        hasher.update(file_content);
        let expected_hash = format!("{:x}", hasher.finalize());

        assert!(BitTorrentHandler::verify_assembled_file_integrity(&file_path, &expected_hash).is_ok());
    }

    #[test]
    fn test_verify_assembled_file_integrity_mismatch() {
        let temp_dir = tempdir().unwrap();
        let file_content = b"This is a test file for integrity verification.";
        let file_path = create_test_file(temp_dir.path(), "mismatched_file.txt", std::str::from_utf8(file_content).unwrap());

        let wrong_hash = "abcdef1234567890abcdef1234567890abcdef12".to_string(); // A deliberately wrong hash

        assert!(BitTorrentHandler::verify_assembled_file_integrity(&file_path, &wrong_hash).is_err());
    }

    #[test]
    fn test_verify_assembled_file_integrity_nonexistent() {
        let temp_dir = tempdir().unwrap();
        let nonexistent_path = temp_dir.path().join("nonexistent.txt");
        let dummy_hash = "abcdef1234567890abcdef1234567890abcdef12".to_string();

        assert!(BitTorrentHandler::verify_assembled_file_integrity(&nonexistent_path, &dummy_hash).is_err());
    }

    // Integration Tests for Download Functionality
    #[tokio::test]
    async fn test_download_valid_magnet_link() {
        let temp_dir = tempdir().unwrap();
        let handler = BitTorrentHandler::new(temp_dir.path().to_path_buf());
        
        let magnet = "magnet:?xt=urn:btih:1234567890abcdef1234567890abcdef12345678&dn=test_file";
        let result = handler.download(magnet).await;
        
        // Should succeed (simulation)
        assert!(result.is_ok());
        
        // Should create download directory
        assert!(temp_dir.path().exists());
    }

    #[tokio::test]
    async fn test_download_invalid_magnet_link() {
        let temp_dir = tempdir().unwrap();
        let handler = BitTorrentHandler::new(temp_dir.path().to_path_buf());
        
        let invalid_magnet = "magnet:?xt=urn:btih:invalid_hash&dn=test";
        let result = handler.download(invalid_magnet).await;
        
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid magnet link format"));
    }

    #[tokio::test]
    async fn test_download_existing_torrent_file() {
        let temp_dir = tempdir().unwrap();
        let handler = BitTorrentHandler::new(temp_dir.path().to_path_buf());
        
        let torrent_path = create_test_file(temp_dir.path(), "test.torrent", "fake torrent content");
        let result = handler.download(torrent_path.to_str().unwrap()).await;
        
        // Should succeed (simulation and verification)
        assert!(result.is_ok());

        // Check if the dummy file was created and verified (using the simulated info hash)
        let simulated_info_hash = "0123456789abcdef0123456789abcdef01234567".to_string();
        let dummy_file_name = format!("{}.bin", simulated_info_hash);
        let dummy_file_path = temp_dir.path().join(&dummy_file_name);
        assert!(dummy_file_path.exists());
    }
    #[tokio::test]
    async fn test_download_unsupported_identifier() {
        let temp_dir = tempdir().unwrap();
        let handler = BitTorrentHandler::new(temp_dir.path().to_path_buf());
        
        let result = handler.download("http://example.com/file.zip").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unsupported identifier format"));
    }

    // Integration Tests for Seeding Functionality
    #[tokio::test]
    async fn test_seed_valid_file() {
        let temp_dir = tempdir().unwrap();
        let handler = BitTorrentHandler::new(temp_dir.path().to_path_buf());
        
        let file_content = "Content to be seeded";
        let file_path = create_test_file(temp_dir.path(), "seed_test.txt", file_content);
        
        let result = handler.seed(file_path.to_str().unwrap()).await;
        assert!(result.is_ok());
        
        let magnet_link = result.unwrap();
        assert!(magnet_link.starts_with("magnet:?xt=urn:btih:"));
        assert!(magnet_link.contains("seed_test.txt"));
    }

    #[tokio::test]
    async fn test_seed_nonexistent_file() {
        let temp_dir = tempdir().unwrap();
        let handler = BitTorrentHandler::new(temp_dir.path().to_path_buf());
        
        let result = handler.seed("/nonexistent/file.txt").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("File does not exist"));
    }

    // Integration Tests for Directory Management
    #[tokio::test]
    async fn test_download_directory_creation() {
        let temp_dir = tempdir().unwrap();
        let download_dir = temp_dir.path().join("downloads");
        
        // Ensure directory doesn't exist initially
        assert!(!download_dir.exists());
        
        let handler = BitTorrentHandler::new(download_dir.clone());
        let magnet = "magnet:?xt=urn:btih:1234567890abcdef1234567890abcdef12345678";
        
        let result = handler.download(magnet).await;
        assert!(result.is_ok());
        
        // Directory should be created
        assert!(download_dir.exists());
        assert!(download_dir.is_dir());
    }

    // Edge Case Tests
    #[tokio::test]
    async fn test_empty_file_seeding() {
        let temp_dir = tempdir().unwrap();
        let handler = BitTorrentHandler::new(temp_dir.path().to_path_buf());
        
        let file_path = create_test_file(temp_dir.path(), "empty.txt", "");
        let result = handler.seed(file_path.to_str().unwrap()).await;
        
        assert!(result.is_ok());
        let magnet_link = result.unwrap();
        assert!(magnet_link.contains("empty.txt"));
    }

    #[tokio::test]
    async fn test_large_filename_handling() {
        let temp_dir = tempdir().unwrap();
        let handler = BitTorrentHandler::new(temp_dir.path().to_path_buf());
        
        let long_name = "a".repeat(200) + ".txt";
        let file_path = create_test_file(temp_dir.path(), &long_name, "content");
        
        let result = handler.seed(file_path.to_str().unwrap()).await;
        assert!(result.is_ok());
        
        let magnet_link = result.unwrap();
        // URL encoding should handle long filenames
        assert!(magnet_link.contains("&dn="));
    }

    #[tokio::test]
    async fn test_special_characters_in_filename() {
        let temp_dir = tempdir().unwrap();
        let handler = BitTorrentHandler::new(temp_dir.path().to_path_buf());
        
        let special_name = "test file with spaces & symbols!@#.txt";
        let file_path = create_test_file(temp_dir.path(), special_name, "content");
        
        let result = handler.seed(file_path.to_str().unwrap()).await;
        assert!(result.is_ok());
        
        let magnet_link = result.unwrap();
        // Should be properly URL encoded
        assert!(magnet_link.contains("&dn="));
        assert!(!magnet_link.contains(" ")); // Spaces should be encoded
    }
}