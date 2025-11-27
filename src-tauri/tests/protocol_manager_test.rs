use chiral_network::protocols::{
    ProtocolManager,
    traits::{
        DownloadHandle, DownloadOptions, DownloadProgress, ProtocolCapabilities, ProtocolError,
        SeedOptions, SeedingInfo, ProtocolHandler
    },
    seeding::SeedingRegistry,
    HttpProtocolHandler,
    FtpProtocolHandler,
};

use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tokio::fs;

#[test]
fn test_protocol_identification() {
    let http = HttpProtocolHandler::new().unwrap();
    let ftp = FtpProtocolHandler::new();

    // HTTP
    assert!(http.supports("http://example.com/file.zip"));
    assert!(http.supports("https://example.com/file.zip"));
    assert!(!http.supports("ftp://example.com/file.zip"));

    // FTP
    assert!(ftp.supports("ftp://example.com/file.zip"));
    assert!(ftp.supports("ftps://example.com/file.zip"));
    assert!(!ftp.supports("http://example.com/file.zip"));
}

#[test]
fn test_protocol_names() {
    let http = HttpProtocolHandler::new().unwrap();
    let ftp = FtpProtocolHandler::new();

    assert_eq!(http.name(), "http");
    assert_eq!(ftp.name(), "ftp");
}

#[test]
fn test_protocol_capabilities() {
    let http = HttpProtocolHandler::new().unwrap();
    let ftp = FtpProtocolHandler::new();

    let http_caps = http.capabilities();
    assert!(!http_caps.supports_seeding);
    assert!(http_caps.supports_encryption); // HTTPS

    let ftp_caps = ftp.capabilities();
    assert!(ftp_caps.supports_seeding);
    assert!(ftp_caps.supports_pause_resume);
}

// =========================================================================
// --- New Tests for Seeding ---
// =========================================================================

/// A mock protocol handler for testing the manager
#[derive(Clone)]
struct MockProtocolHandler {
    name: &'static str,
    supports_seeding: bool,
    // Use Arc<Mutex<>> for interior mutability in an async context
    stop_called: Arc<Mutex<bool>>,
}

impl MockProtocolHandler {
    fn new(name: &'static str, supports_seeding: bool) -> Self {
        Self {
            name,
            supports_seeding,
            stop_called: Arc::new(Mutex::new(false)),
        }
    }
}

#[async_trait]
impl ProtocolHandler for MockProtocolHandler {
    fn name(&self) -> &'static str {
        self.name
    }
    fn supports(&self, _identifier: &str) -> bool {
        true // Not used in these tests
    }

    async fn download(
        &self,
        identifier: &str,
        options: DownloadOptions,
    ) -> Result<DownloadHandle, ProtocolError> {
        let _ = (identifier, options); // Mark as used
        Err(ProtocolError::NotSupported) // Not used
    }

    async fn seed(
        &self,
        file_path: PathBuf,
        options: SeedOptions,
    ) -> Result<SeedingInfo, ProtocolError> {
        let _ = options; // Mark as used
        Ok(SeedingInfo {
            identifier: format!("{}:{}", self.name, file_path.to_string_lossy()),
            file_path,
            protocol: self.name.to_string(),
            active_peers: 0,
            bytes_uploaded: 0,
        })
    }

    async fn stop_seeding(&self, identifier: &str) -> Result<(), ProtocolError> {
        let _ = identifier; // Mark as used
        let mut stop_called = self.stop_called.lock().unwrap();
        *stop_called = true;
        Ok(())
    }

    async fn pause_download(&self, identifier: &str) -> Result<(), ProtocolError> {
        let _ = identifier; // Mark as used
        Err(ProtocolError::NotSupported)
    }

    async fn resume_download(&self, identifier: &str) -> Result<(), ProtocolError> {
        let _ = identifier; // Mark as used
        Err(ProtocolError::NotSupported)
    }

    async fn cancel_download(&self, identifier: &str) -> Result<(), ProtocolError> {
        let _ = identifier; // Mark as used
        Err(ProtocolError::NotSupported)
    }

    async fn get_download_progress(
        &self,
        identifier: &str,
    ) -> Result<DownloadProgress, ProtocolError> {
        let _ = identifier; // Mark as used
        Err(ProtocolError::NotSupported)
    }

    async fn list_seeding(&self) -> Result<Vec<SeedingInfo>, ProtocolError> {
        Ok(Vec::new())
    }
    
    fn capabilities(&self) -> ProtocolCapabilities {
        ProtocolCapabilities {
            supports_seeding: self.supports_seeding,
            ..Default::default()
        }
    }
}

#[tokio::test]
async fn test_calculate_file_hash() {
    let manager = ProtocolManager::new();
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("test_hash.txt");
    fs::write(&file_path, "hello world").await.unwrap();

    let hash = manager.calculate_file_hash(&file_path).await.unwrap();
    // SHA-256 hash of "hello world"
    assert_eq!(
        hash,
        "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
    );
}

#[tokio::test]
async fn test_seeding_registry_add_list_remove() {
    let registry = SeedingRegistry::new();
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("test_file.txt");
    fs::write(&file_path, "test data").await.unwrap();

    let file_hash = "test_hash_123".to_string();
    let info = SeedingInfo {
        identifier: "magnet:?xt=urn:btih:123".to_string(),
        file_path: file_path.clone(),
        protocol: "bittorrent".to_string(),
        active_peers: 0,
        bytes_uploaded: 0,
    };

    // Add
    registry
        .add_seeding(
            file_hash.clone(),
            file_path.clone(),
            "bittorrent".to_string(),
            info,
        )
        .await
        .unwrap();

    // List
    let entries = registry.list_all().await;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].file_hash, file_hash);
    assert_eq!(entries[0].protocols.len(), 1);
    assert!(entries[0].protocols.contains_key("bittorrent"));

    // Remove
    registry.remove_seeding(&file_hash).await;
    let entries = registry.list_all().await;
    assert_eq!(entries.len(), 0);
}

#[tokio::test]
async fn test_protocol_manager_seed_and_stop_all() {
    let mut manager = ProtocolManager::new();

    // Create a mock handler that supports seeding
    let mock_bt = MockProtocolHandler::new("bittorrent", true);
    let stop_called_flag = mock_bt.stop_called.clone();
    manager.register(Box::new(mock_bt));

    // Create a mock handler that does NOT support seeding
    let mock_http = MockProtocolHandler::new("http", false);
    manager.register(Box::new(mock_http));

    // Create a temp file
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("seed_test.txt");
    fs::write(&file_path, "seed this file").await.unwrap();
    let file_hash = manager.calculate_file_hash(&file_path).await.unwrap();

    // 1. Seed the file on bittorrent and http
    let seed_options = SeedOptions::default();
    let protocols = vec!["bittorrent".to_string(), "http".to_string()];
    let results = manager
        .seed_file_multi_protocol(file_path.clone(), protocols, seed_options)
        .await
        .unwrap();

    // Verify only bittorrent succeeded
    assert_eq!(results.len(), 1);
    assert!(results.contains_key("bittorrent"));
    assert!(!results.contains_key("http"));

    // 2. List seeding files
    let seeding_files = manager.list_seeding_files().await;
    assert_eq!(seeding_files.len(), 1);
    assert_eq!(seeding_files[0].file_hash, file_hash);
    assert!(seeding_files[0].protocols.contains_key("bittorrent"));

    // 3. Stop seeding
    assert_eq!(*stop_called_flag.lock().unwrap(), false); // Not called yet
    manager.stop_seeding_all(&file_hash).await.unwrap();

    // 4. Verify seeding stopped
    let seeding_files_after_stop = manager.list_seeding_files().await;
    assert_eq!(seeding_files_after_stop.len(), 0);

    // Verify the mock handler's stop_seeding was called
    assert_eq!(*stop_called_flag.lock().unwrap(), true);
}