use std::path::{Path, PathBuf};
use std::fs;
use std::time::{SystemTime, Duration};
use serde::{Serialize, Deserialize};
use anyhow::{Result, Context};

/// Configuration for storage management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Maximum total storage size in GB
    pub max_storage_size_gb: u64,
    /// Enable automatic cleanup
    pub auto_cleanup: bool,
    /// Cleanup threshold percentage (0-100)
    pub cleanup_threshold: u64,
    /// Cache size limit in MB
    pub cache_size_mb: u64,
    /// Download directory path
    pub download_path: PathBuf,
    /// Blockstore database path
    pub blockstore_path: PathBuf,
    /// Temporary files path
    pub temp_path: PathBuf,
    /// Chunk storage path
    pub chunk_storage_path: PathBuf,
}

/// Storage usage information across all locations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageUsage {
    /// Total size in bytes
    pub total_bytes: u64,
    /// Downloads directory size
    pub downloads_bytes: u64,
    /// Blockstore size
    pub blockstore_bytes: u64,
    /// Temporary files size
    pub temp_bytes: u64,
    /// Chunk storage size
    pub chunk_storage_bytes: u64,
    /// Available disk space in bytes
    pub available_bytes: u64,
    /// Timestamp of calculation
    pub timestamp: SystemTime,
}

impl StorageUsage {
    /// Calculate total usage as percentage of max allowed
    pub fn usage_percentage(&self, max_gb: u64) -> f64 {
        let max_bytes = max_gb * 1024 * 1024 * 1024;
        (self.total_bytes as f64 / max_bytes as f64) * 100.0
    }

    /// Check if cleanup is needed based on threshold
    pub fn needs_cleanup(&self, max_gb: u64, threshold: u64) -> bool {
        self.usage_percentage(max_gb) >= threshold as f64
    }

    /// Format bytes to human-readable string
    pub fn format_bytes(bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;
        const TB: u64 = GB * 1024;

        if bytes >= TB {
            format!("{:.2} TB", bytes as f64 / TB as f64)
        } else if bytes >= GB {
            format!("{:.2} GB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.2} MB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.2} KB", bytes as f64 / KB as f64)
        } else {
            format!("{} B", bytes)
        }
    }
}

/// Report of cleanup operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupReport {
    /// Number of files deleted
    pub files_deleted: usize,
    /// Total bytes freed
    pub bytes_freed: u64,
    /// Cleanup duration in milliseconds
    pub duration_ms: u64,
    /// Errors encountered
    pub errors: Vec<String>,
    /// Breakdown by location
    pub downloads_freed: u64,
    pub temp_freed: u64,
    pub orphaned_freed: u64,
}

impl CleanupReport {
    pub fn new() -> Self {
        Self {
            files_deleted: 0,
            bytes_freed: 0,
            duration_ms: 0,
            errors: Vec::new(),
            downloads_freed: 0,
            temp_freed: 0,
            orphaned_freed: 0,
        }
    }

    pub fn add_error(&mut self, error: String) {
        self.errors.push(error);
    }
}

/// Information about a file for cleanup purposes
#[derive(Debug, Clone)]
pub struct FileInfo {
    pub path: PathBuf,
    pub size: u64,
    pub modified: SystemTime,
    pub accessed: SystemTime,
}

impl FileInfo {
    /// Create FileInfo from a path
    pub fn from_path(path: &Path) -> Result<Self> {
        let metadata = fs::metadata(path)
            .with_context(|| format!("Failed to get metadata for {:?}", path))?;

        Ok(Self {
            path: path.to_path_buf(),
            size: metadata.len(),
            modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            accessed: metadata.accessed().unwrap_or(SystemTime::UNIX_EPOCH),
        })
    }

    /// Check if file is older than duration
    pub fn is_older_than(&self, duration: Duration) -> bool {
        if let Ok(elapsed) = self.modified.elapsed() {
            elapsed > duration
        } else {
            false
        }
    }
}

/// Main storage manager
pub struct StorageManager {
    config: StorageConfig,
}

impl StorageManager {
    /// Create a new storage manager with configuration
    pub fn new(config: StorageConfig) -> Self {
        Self { config }
    }

    /// Calculate current storage usage across all locations
    pub async fn calculate_usage(&self) -> Result<StorageUsage> {
        let downloads_bytes = calculate_directory_size(&self.config.download_path).await?;
        let blockstore_bytes = calculate_directory_size(&self.config.blockstore_path).await?;
        let temp_bytes = calculate_directory_size(&self.config.temp_path).await?;
        let chunk_storage_bytes = calculate_directory_size(&self.config.chunk_storage_path).await?;

        let total_bytes = downloads_bytes + blockstore_bytes + temp_bytes + chunk_storage_bytes;

        // Get available disk space
        let available_bytes = get_available_space(&self.config.download_path)?;

        Ok(StorageUsage {
            total_bytes,
            downloads_bytes,
            blockstore_bytes,
            temp_bytes,
            chunk_storage_bytes,
            available_bytes,
            timestamp: SystemTime::now(),
        })
    }

    /// Check if cleanup is needed and perform it if enabled
    pub async fn check_and_cleanup(&self) -> Result<Option<CleanupReport>> {
        let usage = self.calculate_usage().await?;

        if !self.config.auto_cleanup {
            return Ok(None);
        }

        if usage.needs_cleanup(self.config.max_storage_size_gb, self.config.cleanup_threshold) {
            let report = self.perform_cleanup(&usage).await?;
            Ok(Some(report))
        } else {
            Ok(None)
        }
    }

    /// Perform cleanup operations
    async fn perform_cleanup(&self, usage: &StorageUsage) -> Result<CleanupReport> {
        let start = SystemTime::now();
        let mut report = CleanupReport::new();

        // Calculate how much space we need to free
        let target_percentage = (self.config.cleanup_threshold - 10).max(50); // Clean to 10% below threshold
        let max_bytes = self.config.max_storage_size_gb * 1024 * 1024 * 1024;
        let target_bytes = (max_bytes as f64 * target_percentage as f64 / 100.0) as u64;
        let bytes_to_free = usage.total_bytes.saturating_sub(target_bytes);

        tracing::info!(
            "Starting cleanup: current usage {}, target {}, need to free {}",
            StorageUsage::format_bytes(usage.total_bytes),
            StorageUsage::format_bytes(target_bytes),
            StorageUsage::format_bytes(bytes_to_free)
        );

        // Cleanup strategy (in order of priority):
        // 1. Orphaned temporary files (safest)
        // 2. Old temporary files
        // 3. Oldest completed downloads (LRU)

        let mut freed = 0u64;

        // Step 1: Clean orphaned temp files
        match self.cleanup_orphaned_temp_files().await {
            Ok(bytes) => {
                report.temp_freed += bytes;
                freed += bytes;
                tracing::info!("Cleaned orphaned temp files: {}", StorageUsage::format_bytes(bytes));
            }
            Err(e) => {
                report.add_error(format!("Failed to clean temp files: {}", e));
            }
        }

        // Step 2: Clean old temp files if we need more space
        if freed < bytes_to_free {
            match self.cleanup_old_temp_files(Duration::from_secs(86400)).await { // 24 hours
                Ok(bytes) => {
                    report.temp_freed += bytes;
                    freed += bytes;
                    tracing::info!("Cleaned old temp files: {}", StorageUsage::format_bytes(bytes));
                }
                Err(e) => {
                    report.add_error(format!("Failed to clean old temp files: {}", e));
                }
            }
        }

        // Step 3: Clean orphaned part files
        if freed < bytes_to_free {
            match self.cleanup_orphaned_part_files().await {
                Ok(bytes) => {
                    report.orphaned_freed += bytes;
                    freed += bytes;
                    tracing::info!("Cleaned orphaned part files: {}", StorageUsage::format_bytes(bytes));
                }
                Err(e) => {
                    report.add_error(format!("Failed to clean orphaned files: {}", e));
                }
            }
        }

        // Step 4: LRU cleanup of completed downloads if still need space
        if freed < bytes_to_free {
            let remaining = bytes_to_free - freed;
            match self.cleanup_old_downloads(remaining).await {
                Ok(bytes) => {
                    report.downloads_freed += bytes;
                    freed += bytes;
                    tracing::info!("Cleaned old downloads: {}", StorageUsage::format_bytes(bytes));
                }
                Err(e) => {
                    report.add_error(format!("Failed to clean downloads: {}", e));
                }
            }
        }

        report.bytes_freed = freed;

        if let Ok(elapsed) = start.elapsed() {
            report.duration_ms = elapsed.as_millis() as u64;
        }

        tracing::info!(
            "Cleanup completed: freed {}, deleted {} files, took {}ms",
            StorageUsage::format_bytes(report.bytes_freed),
            report.files_deleted,
            report.duration_ms
        );

        Ok(report)
    }

    /// Clean up orphaned temporary files
    async fn cleanup_orphaned_temp_files(&self) -> Result<u64> {
        let mut bytes_freed = 0u64;

        if !self.config.temp_path.exists() {
            return Ok(0);
        }

        let entries = fs::read_dir(&self.config.temp_path)?;

        for entry in entries.flatten() {
            let path = entry.path();

            // Clean up .tmp files that don't have corresponding active transfers
            if path.extension().and_then(|s| s.to_str()) == Some("tmp") {
                if let Ok(metadata) = fs::metadata(&path) {
                    bytes_freed += metadata.len();
                    if let Err(e) = fs::remove_file(&path) {
                        tracing::warn!("Failed to remove temp file {:?}: {}", path, e);
                    }
                }
            }

            // Clean up .bitmap files without corresponding .tmp
            if path.extension().and_then(|s| s.to_str()) == Some("bitmap") {
                let tmp_path = path.with_extension("tmp");
                if !tmp_path.exists() {
                    if let Ok(metadata) = fs::metadata(&path) {
                        bytes_freed += metadata.len();
                        if let Err(e) = fs::remove_file(&path) {
                            tracing::warn!("Failed to remove bitmap file {:?}: {}", path, e);
                        }
                    }
                }
            }
        }

        Ok(bytes_freed)
    }

    /// Clean up old temporary files
    async fn cleanup_old_temp_files(&self, age: Duration) -> Result<u64> {
        let mut bytes_freed = 0u64;

        if !self.config.temp_path.exists() {
            return Ok(0);
        }

        let entries = fs::read_dir(&self.config.temp_path)?;

        for entry in entries.flatten() {
            let path = entry.path();

            if let Ok(file_info) = FileInfo::from_path(&path) {
                if file_info.is_older_than(age) {
                    bytes_freed += file_info.size;
                    if let Err(e) = fs::remove_file(&path) {
                        tracing::warn!("Failed to remove old temp file {:?}: {}", path, e);
                    }
                }
            }
        }

        Ok(bytes_freed)
    }

    /// Clean up orphaned .part files (older than 7 days with no corresponding .meta.json)
    async fn cleanup_orphaned_part_files(&self) -> Result<u64> {
        let mut bytes_freed = 0u64;

        if !self.config.download_path.exists() {
            return Ok(0);
        }

        let entries = fs::read_dir(&self.config.download_path)?;
        let stale_threshold = Duration::from_secs(7 * 86400); // 7 days

        for entry in entries.flatten() {
            let path = entry.path();

            // Look for .part files
            if path.extension().and_then(|s| s.to_str()) == Some("part") {
                let meta_path = path.with_extension("part.meta.json");

                // If no metadata file exists, it's orphaned
                if !meta_path.exists() {
                    if let Ok(file_info) = FileInfo::from_path(&path) {
                        // Only delete if older than threshold
                        if file_info.is_older_than(stale_threshold) {
                            bytes_freed += file_info.size;
                            if let Err(e) = fs::remove_file(&path) {
                                tracing::warn!("Failed to remove orphaned part file {:?}: {}", path, e);
                            } else {
                                tracing::info!("Removed orphaned part file: {:?}", path);
                            }
                        }
                    }
                }
            }

            // Clean up orphaned .meta.json files
            if path.to_string_lossy().ends_with(".meta.json") {
                let part_path = PathBuf::from(path.to_string_lossy().replace(".meta.json", ""));

                if !part_path.exists() {
                    if let Ok(file_info) = FileInfo::from_path(&path) {
                        if file_info.is_older_than(stale_threshold) {
                            bytes_freed += file_info.size;
                            if let Err(e) = fs::remove_file(&path) {
                                tracing::warn!("Failed to remove orphaned meta file {:?}: {}", path, e);
                            } else {
                                tracing::info!("Removed orphaned meta file: {:?}", path);
                            }
                        }
                    }
                }
            }
        }

        Ok(bytes_freed)
    }

    /// Clean up old completed downloads using LRU policy
    async fn cleanup_old_downloads(&self, bytes_needed: u64) -> Result<u64> {
        let mut bytes_freed = 0u64;

        if !self.config.download_path.exists() {
            return Ok(0);
        }

        // Collect all completed files (not .part, not .meta.json)
        let mut files: Vec<FileInfo> = Vec::new();

        let entries = fs::read_dir(&self.config.download_path)?;
        for entry in entries.flatten() {
            let path = entry.path();

            // Skip .part and .meta.json files
            let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if file_name.ends_with(".part") || file_name.ends_with(".meta.json") {
                continue;
            }

            if path.is_file() {
                if let Ok(file_info) = FileInfo::from_path(&path) {
                    files.push(file_info);
                }
            }
        }

        // Sort by last accessed time (LRU - oldest first)
        files.sort_by_key(|f| f.accessed);

        // Delete oldest files until we've freed enough space
        for file in files {
            if bytes_freed >= bytes_needed {
                break;
            }

            bytes_freed += file.size;
            if let Err(e) = fs::remove_file(&file.path) {
                tracing::warn!("Failed to remove old download {:?}: {}", file.path, e);
            } else {
                tracing::info!("Removed old download (LRU): {:?}", file.path);
            }
        }

        Ok(bytes_freed)
    }

    /// Manual cleanup trigger (ignores auto_cleanup setting)
    pub async fn force_cleanup(&self) -> Result<CleanupReport> {
        let usage = self.calculate_usage().await?;
        self.perform_cleanup(&usage).await
    }
}

/// Calculate the total size of a directory recursively
fn calculate_directory_size_sync(path: &Path) -> Result<u64> {
    if !path.exists() {
        return Ok(0);
    }

    let mut total_size = 0u64;

    if path.is_file() {
        if let Ok(metadata) = fs::metadata(path) {
            return Ok(metadata.len());
        }
        return Ok(0);
    }

    let entries = fs::read_dir(path)
        .with_context(|| format!("Failed to read directory {:?}", path))?;

    for entry in entries.flatten() {
        let entry_path = entry.path();

        if entry_path.is_file() {
            if let Ok(metadata) = fs::metadata(&entry_path) {
                total_size += metadata.len();
            }
        } else if entry_path.is_dir() {
            // Recursively calculate subdirectory size
            total_size += calculate_directory_size_sync(&entry_path)?;
        }
    }

    Ok(total_size)
}

/// Async wrapper for calculate_directory_size_sync
async fn calculate_directory_size(path: &Path) -> Result<u64> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        calculate_directory_size_sync(&path)
    }).await
        .map_err(|e| anyhow::anyhow!("Task join error: {}", e))?
}

/// Get available disk space for a path
fn get_available_space(path: &Path) -> Result<u64> {
    // Use fs2 crate for cross-platform available space
    fs2::available_space(path)
        .with_context(|| format!("Failed to get available space for {:?}", path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(StorageUsage::format_bytes(512), "512 B");
        assert_eq!(StorageUsage::format_bytes(1024), "1.00 KB");
        assert_eq!(StorageUsage::format_bytes(1024 * 1024), "1.00 MB");
        assert_eq!(StorageUsage::format_bytes(1024 * 1024 * 1024), "1.00 GB");
    }

    #[test]
    fn test_usage_percentage() {
        let usage = StorageUsage {
            total_bytes: 50 * 1024 * 1024 * 1024, // 50 GB
            downloads_bytes: 0,
            blockstore_bytes: 0,
            temp_bytes: 0,
            chunk_storage_bytes: 0,
            available_bytes: 0,
            timestamp: SystemTime::now(),
        };

        assert_eq!(usage.usage_percentage(100), 50.0);
    }

    #[test]
    fn test_needs_cleanup() {
        let usage = StorageUsage {
            total_bytes: 95 * 1024 * 1024 * 1024, // 95 GB
            downloads_bytes: 0,
            blockstore_bytes: 0,
            temp_bytes: 0,
            chunk_storage_bytes: 0,
            available_bytes: 0,
            timestamp: SystemTime::now(),
        };

        assert!(usage.needs_cleanup(100, 90)); // 95% > 90% threshold
        assert!(!usage.needs_cleanup(100, 96)); // 95% < 96% threshold
    }
}
