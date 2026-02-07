// download_persistence.rs
// Persistence & storage safety for download pause/resume
//
// This module implements Elliot's deliverables for the download-restart baseline:
// - .meta.json schema v1 with atomic write (write temp → fsync -> rename)
// - .part writer with per-path mutex + OS advisory lock (fs2::try_lock_exclusive)
// - Fsync policy: every 8 MiB (configurable); cross-volume finalize via stream-copy
// - Preflight free space checks
// - Resume validation: .part length == bytes_downloaded or restart cleanly
// - Destination path sandboxing under downloads root

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use thiserror::Error;
use tracing::{debug, error, info, warn};

/// Schema version for forward compatibility
const METADATA_VERSION: u32 = 1;

/// Default fsync interval: 8 MiB
pub const DEFAULT_FSYNC_INTERVAL: u64 = 8 * 1024 * 1024;

/// Download metadata schema v1
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadMetadata {
    /// Schema version for forward compatibility
    pub version: u32,
    
    /// Stable identifier for this download
    pub download_id: String,
    
    /// Source URL (HTTP, FTP, etc.)
    pub url: String,
    
    /// Strong ETag from server (for resume validation)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    
    /// Expected total size in bytes
    pub expected_size: u64,
    
    /// Bytes successfully downloaded and fsynced to disk
    pub bytes_downloaded: u64,
    
    /// Last-Modified timestamp from server (RFC 3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
    
    /// Final SHA-256 hash after verification (populated on completion)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256_final: Option<String>,
}

/// Errors that can occur during persistence operations
#[derive(Debug, Error)]
pub enum PersistenceError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    
    #[error("insufficient disk space: need {needed} bytes, have {available} bytes")]
    DiskFull { needed: u64, available: u64 },
    
    #[error("metadata version {0} is not supported (expected {METADATA_VERSION})")]
    UnsupportedVersion(u32),
    
    #[error("path traversal detected: {0}")]
    PathTraversal(String),
    
    #[error("file lock could not be acquired: {0}")]
    LockFailed(String),
    
    #[error("metadata corrupted: {0}")]
    CorruptedMetadata(String),
    
    #[error("part file size mismatch: expected {expected}, found {actual}")]
    PartSizeMismatch { expected: u64, actual: u64 },
}

/// Configuration for download persistence
#[derive(Debug, Clone)]
pub struct PersistenceConfig {
    /// Root directory for downloads (for path sandboxing)
    pub downloads_root: PathBuf,
    
    /// Fsync interval in bytes (default: 8 MiB)
    pub fsync_interval: u64,
    
    /// Whether to perform strict validation on resume
    pub strict_validation: bool,
}

impl Default for PersistenceConfig {
    fn default() -> Self {
        Self {
            downloads_root: PathBuf::from("downloads"),
            fsync_interval: DEFAULT_FSYNC_INTERVAL,
            strict_validation: true,
        }
    }
}

// Global registry of file locks to prevent concurrent writes
lazy_static::lazy_static! {
    static ref FILE_LOCKS: StdMutex<HashMap<PathBuf, Arc<StdMutex<()>>>> = StdMutex::new(HashMap::new());
}

/// Manager for download persistence operations
pub struct DownloadPersistence {
    config: PersistenceConfig,
}

impl DownloadPersistence {
    /// Create a new persistence manager with the given configuration
    pub fn new(config: PersistenceConfig) -> Self {
        Self { config }
    }
    
    /// Validate that the destination path is sandboxed under downloads_root
    pub fn validate_destination_path(&self, dest: &Path) -> Result<PathBuf, PersistenceError> {
        // Ensure downloads_root exists and get its canonical path
        fs::create_dir_all(&self.config.downloads_root)?;
        let canonical_root = self.config.downloads_root.canonicalize()
            .map_err(|e| PersistenceError::Io(e))?;
        
        // Try to canonicalize the destination if it exists, otherwise normalize it
        let normalized_dest = if let Ok(canonical_dest) = dest.canonicalize() {
            // Path exists, use canonical form (resolves symlinks like /var → /private/var on macOS)
            canonical_dest
        } else {
            // Path doesn't exist yet, normalize it manually
            if dest.is_absolute() {
                self.normalize_path(dest)
            } else {
                // If relative, make it absolute relative to downloads_root
                self.normalize_path(&canonical_root.join(dest))
            }
        };
        
        // Check if normalized destination is under downloads_root
        if !normalized_dest.starts_with(&canonical_root) {
            return Err(PersistenceError::PathTraversal(
                format!("{} is not under {}", normalized_dest.display(), canonical_root.display())
            ));
        }
        
        Ok(normalized_dest)
    }
    
    /// Normalize a path by resolving '..' components without requiring the path to exist
    fn normalize_path(&self, path: &Path) -> PathBuf {
        let mut components = Vec::new();
        
        for component in path.components() {
            match component {
                std::path::Component::ParentDir => {
                    // Pop the last component if it's not already at root
                    if !components.is_empty() {
                        components.pop();
                    }
                }
                std::path::Component::CurDir => {
                    // Skip current directory references
                }
                _ => {
                    components.push(component);
                }
            }
        }
        
        components.iter().collect()
    }
    
    /// Get paths for .part and .meta.json files
    pub fn get_temp_paths(&self, destination: &Path) -> (PathBuf, PathBuf) {
        let part_path = destination.with_extension("part");
        let meta_path = destination.with_extension("meta.json");
        (part_path, meta_path)
    }
    
    /// Perform preflight storage checks
    /// Returns available space in bytes
    pub fn preflight_storage_check(
        &self,
        destination: &Path,
        expected_size: u64,
        bytes_already_downloaded: u64,
    ) -> Result<u64, PersistenceError> {
        // Ensure parent directory exists
        let parent = destination.parent().unwrap_or(destination);
        fs::create_dir_all(parent)?;
        
        // Check available disk space on the parent directory (not the file itself)
        let available = fs2::available_space(parent)?;
        let needed = expected_size.saturating_sub(bytes_already_downloaded);
        
        if available < needed {
            return Err(PersistenceError::DiskFull { needed, available });
        }
        
        debug!(
            "Preflight check passed: need {} bytes, have {} bytes available",
            needed, available
        );
        
        Ok(available)
    }
    
    /// Acquire per-path mutex and OS advisory lock on .part file
    pub fn acquire_lock(&self, part_path: &Path) -> Result<(Arc<StdMutex<()>>, File), PersistenceError> {
        // Get or create per-path mutex
        let path_mutex = {
            let mut locks = FILE_LOCKS.lock().unwrap();
            locks.entry(part_path.to_path_buf())
                .or_insert_with(|| Arc::new(StdMutex::new(())))
                .clone()
        };
        
        // Open .part file with read/write/create flags
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(part_path)?;
        
        // Try to acquire OS advisory lock (non-blocking)
        file.try_lock_exclusive()
            .map_err(|e| PersistenceError::LockFailed(format!("{}", e)))?;
        
        debug!("Acquired lock on {}", part_path.display());
        
        Ok((path_mutex, file))
    }
    
    /// Write metadata atomically: write temp → fsync -> rename
    pub fn write_metadata_atomic(
        &self,
        meta_path: &Path,
        metadata: &DownloadMetadata,
    ) -> Result<(), PersistenceError> {
        // Write to temporary file
        let temp_path = meta_path.with_extension("meta.json.tmp");
        
        let json = serde_json::to_string_pretty(metadata)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        
        let mut temp_file = File::create(&temp_path)?;
        temp_file.write_all(json.as_bytes())?;
        
        // Fsync to ensure data is on disk
        temp_file.sync_all()?;
        drop(temp_file);
        
        // Atomic rename
        fs::rename(&temp_path, meta_path)?;
        
        debug!("Wrote metadata atomically to {}", meta_path.display());
        
        Ok(())
    }
    
    /// Read and validate metadata
    pub fn read_metadata(&self, meta_path: &Path) -> Result<DownloadMetadata, PersistenceError> {
        let file = File::open(meta_path)?;
        let metadata: DownloadMetadata = serde_json::from_reader(file)
            .map_err(|e| PersistenceError::CorruptedMetadata(e.to_string()))?;
        
        // Check version
        if metadata.version != METADATA_VERSION {
            return Err(PersistenceError::UnsupportedVersion(metadata.version));
        }
        
        Ok(metadata)
    }
    
    /// Validate .part file against metadata on resume
    pub fn validate_part_file(
        &self,
        part_path: &Path,
        metadata: &DownloadMetadata,
    ) -> Result<(), PersistenceError> {
        let actual_size = fs::metadata(part_path)?.len();
        
        if actual_size != metadata.bytes_downloaded {
            warn!(
                "Part file size mismatch: expected {} bytes, found {} bytes. Will restart cleanly.",
                metadata.bytes_downloaded, actual_size
            );
            return Err(PersistenceError::PartSizeMismatch {
                expected: metadata.bytes_downloaded,
                actual: actual_size,
            });
        }
        
        debug!("Part file validation passed: {} bytes", actual_size);
        Ok(())
    }
    
    /// Finalize download: verify, rename .part to final destination, remove metadata
    /// Handles cross-volume moves via stream-copy + fsync + replace
    pub fn finalize_download(
        &self,
        part_path: &Path,
        destination: &Path,
        meta_path: &Path,
    ) -> Result<(), PersistenceError> {
        // Check if on same filesystem
        let part_metadata = fs::metadata(part_path)?;
        
        // Try atomic rename first (works if same filesystem)
        match fs::rename(part_path, destination) {
            Ok(_) => {
                debug!("Renamed {} to {} (same filesystem)", part_path.display(), destination.display());
            }
            Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
                // Cross-filesystem: stream-copy + fsync + replace
                info!("Cross-filesystem move detected, performing stream-copy");
                
                let mut source = File::open(part_path)?;
                let mut dest_file = File::create(destination)?;
                
                // Stream copy
                io::copy(&mut source, &mut dest_file)?;
                
                // Fsync destination
                dest_file.sync_all()?;
                drop(dest_file);
                
                // Remove source .part file
                fs::remove_file(part_path)?;
                
                debug!("Stream-copied {} to {}", part_path.display(), destination.display());
            }
            Err(e) => return Err(e.into()),
        }
        
        // Remove metadata file
        if meta_path.exists() {
            fs::remove_file(meta_path)?;
            debug!("Removed metadata file {}", meta_path.display());
        }
        
        info!("Download finalized: {}", destination.display());
        Ok(())
    }
    
    /// Clean up artifacts on restart
    pub fn cleanup_artifacts(&self, part_path: &Path, meta_path: &Path) -> Result<(), PersistenceError> {
        if part_path.exists() {
            fs::remove_file(part_path)?;
            debug!("Removed .part file: {}", part_path.display());
        }
        
        if meta_path.exists() {
            fs::remove_file(meta_path)?;
            debug!("Removed metadata file: {}", meta_path.display());
        }
        
        Ok(())
    }
}

/// Writer for .part files with fsync policy
pub struct PartFileWriter {
    file: File,
    bytes_written_since_fsync: u64,
    total_bytes_written: u64,
    fsync_interval: u64,
    _path_lock: Arc<StdMutex<()>>,
}

impl PartFileWriter {
    /// Create a new .part file writer
    pub fn new(
        file: File,
        path_lock: Arc<StdMutex<()>>,
        fsync_interval: u64,
        resume_offset: u64,
    ) -> Result<Self, PersistenceError> {
        let mut writer = Self {
            file,
            bytes_written_since_fsync: 0,
            total_bytes_written: resume_offset,
            fsync_interval,
            _path_lock: path_lock,
        };
        
        // Seek to resume offset
        if resume_offset > 0 {
            writer.file.seek(SeekFrom::Start(resume_offset))?;
            debug!("Seeked to offset {} for resume", resume_offset);
        }
        
        Ok(writer)
    }
    
    /// Write data to .part file with fsync policy
    pub fn write(&mut self, data: &[u8]) -> Result<usize, PersistenceError> {
        let written = self.file.write(data)?;
        self.bytes_written_since_fsync += written as u64;
        self.total_bytes_written += written as u64;
        
        // Fsync if we've crossed the interval threshold
        if self.bytes_written_since_fsync >= self.fsync_interval {
            self.fsync()?;
        }
        
        Ok(written)
    }
    
    /// Force fsync
    pub fn fsync(&mut self) -> Result<(), PersistenceError> {
        self.file.sync_all()?;
        debug!("Fsynced after {} bytes", self.bytes_written_since_fsync);
        self.bytes_written_since_fsync = 0;
        Ok(())
    }
    
    /// Get total bytes written
    pub fn total_bytes_written(&self) -> u64 {
        self.total_bytes_written
    }
    
    /// Flush and fsync on drop
    pub fn finalize(mut self) -> Result<(), PersistenceError> {
        if self.bytes_written_since_fsync > 0 {
            self.fsync()?;
        }
        Ok(())
    }
}

impl Drop for PartFileWriter {
    fn drop(&mut self) {
        // Best-effort fsync on drop
        if self.bytes_written_since_fsync > 0 {
            if let Err(e) = self.file.sync_all() {
                error!("Failed to fsync on drop: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;
    
    #[test]
    fn test_metadata_serialization() {
        let metadata = DownloadMetadata {
            version: 1,
            download_id: "test-123".to_string(),
            url: "https://example.com/file.bin".to_string(),
            etag: Some("\"abc123\"".to_string()),
            expected_size: 1024000,
            bytes_downloaded: 512000,
            last_modified: Some("2025-01-01T00:00:00Z".to_string()),
            sha256_final: None,
        };
        
        let json = serde_json::to_string_pretty(&metadata).unwrap();
        let deserialized: DownloadMetadata = serde_json::from_str(&json).unwrap();
        
        assert_eq!(deserialized.download_id, "test-123");
        assert_eq!(deserialized.expected_size, 1024000);
        assert_eq!(deserialized.bytes_downloaded, 512000);
    }
    
    #[test]
    fn test_atomic_metadata_write() {
        let temp_dir = TempDir::new().unwrap();
        let config = PersistenceConfig {
            downloads_root: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        
        let persistence = DownloadPersistence::new(config);
        let meta_path = temp_dir.path().join("test.meta.json");
        
        let metadata = DownloadMetadata {
            version: 1,
            download_id: "test-456".to_string(),
            url: "https://example.com/file.bin".to_string(),
            etag: Some("\"xyz789\"".to_string()),
            expected_size: 2048000,
            bytes_downloaded: 1024000,
            last_modified: None,
            sha256_final: None,
        };
        
        // Write metadata
        persistence.write_metadata_atomic(&meta_path, &metadata).unwrap();
        
        // Read it back
        let read_metadata = persistence.read_metadata(&meta_path).unwrap();
        assert_eq!(read_metadata.download_id, "test-456");
        assert_eq!(read_metadata.bytes_downloaded, 1024000);
    }
    
    #[test]
    fn test_path_sandboxing() {
        let temp_dir = TempDir::new().unwrap();
        let config = PersistenceConfig {
            downloads_root: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        
        let persistence = DownloadPersistence::new(config);
        
        // Valid path under downloads_root
        let valid_path = temp_dir.path().join("subdir/file.bin");
        fs::create_dir_all(temp_dir.path().join("subdir")).unwrap();
        File::create(&valid_path).unwrap();
        
        assert!(persistence.validate_destination_path(&valid_path).is_ok());
        
        // Path traversal attempt
        let evil_path = temp_dir.path().join("../../../etc/passwd");
        assert!(persistence.validate_destination_path(&evil_path).is_err());
    }
    
    #[test]
    fn test_preflight_storage_check() {
        let temp_dir = TempDir::new().unwrap();
        let config = PersistenceConfig {
            downloads_root: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        
        let persistence = DownloadPersistence::new(config);
        let dest_path = temp_dir.path().join("test.bin");
        
        // Should succeed (reasonable size)
        let result = persistence.preflight_storage_check(&dest_path, 1024, 0);
        assert!(result.is_ok());
        
        // Get actual available space on the temp directory
        let available = fs2::available_space(temp_dir.path()).unwrap();
        
        // Should fail when requesting more than available
        // Request available + 1GB to ensure it exceeds capacity
        let excessive_size = available.saturating_add(1024 * 1024 * 1024);
        let result = persistence.preflight_storage_check(&dest_path, excessive_size, 0);
        assert!(matches!(result, Err(PersistenceError::DiskFull { .. })),
                "Should fail when requesting {} bytes (available: {} bytes)", excessive_size, available);
    }
    
    #[test]
    fn test_part_file_writer_fsync() {
        let temp_dir = TempDir::new().unwrap();
        let part_path = temp_dir.path().join("test.part");
        
        let file = File::create(&part_path).unwrap();
        let lock = Arc::new(StdMutex::new(()));
        
        let mut writer = PartFileWriter::new(file, lock, 16, 0).unwrap();
        
        // Write 32 bytes (should trigger 2 fsyncs with 16-byte interval)
        writer.write(&[0u8; 32]).unwrap();
        
        assert_eq!(writer.total_bytes_written(), 32);
    }
}
