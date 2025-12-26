use std::path::{Path, PathBuf};
use std::fs;
use anyhow::{Result, Context};
use serde::{Serialize, Deserialize};

/// Blockstore statistics and management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockstoreStats {
    /// Total size of blockstore in bytes
    pub total_bytes: u64,
    /// Number of files in blockstore directory
    pub file_count: usize,
    /// Path to blockstore
    pub path: PathBuf,
    /// Whether blockstore exists
    pub exists: bool,
    /// Whether blockstore exceeds configured cache size
    pub exceeds_limit: bool,
    /// Configured cache size limit in MB
    pub cache_limit_mb: u64,
}

impl BlockstoreStats {
    /// Format bytes to human-readable string
    pub fn format_size(&self) -> String {
        format_bytes(self.total_bytes)
    }

    /// Get usage percentage of cache limit
    pub fn usage_percentage(&self) -> f64 {
        let limit_bytes = self.cache_limit_mb * 1024 * 1024;
        (self.total_bytes as f64 / limit_bytes as f64) * 100.0
    }
}

/// Result of blockstore cleanup operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockstoreCleanupReport {
    /// Bytes freed by cleanup
    pub bytes_freed: u64,
    /// Files deleted
    pub files_deleted: usize,
    /// Errors encountered
    pub errors: Vec<String>,
    /// Cleanup method used
    pub method: String,
}

/// Blockstore manager
pub struct BlockstoreManager {
    blockstore_path: PathBuf,
    cache_limit_mb: u64,
}

impl BlockstoreManager {
    /// Create a new blockstore manager
    pub fn new(blockstore_path: PathBuf, cache_limit_mb: u64) -> Self {
        Self {
            blockstore_path,
            cache_limit_mb,
        }
    }

    /// Get blockstore statistics
    pub fn get_stats(&self) -> Result<BlockstoreStats> {
        let exists = self.blockstore_path.exists();

        let (total_bytes, file_count) = if exists {
            (
                calculate_directory_size(&self.blockstore_path)?,
                count_files(&self.blockstore_path)?,
            )
        } else {
            (0, 0)
        };

        let limit_bytes = self.cache_limit_mb * 1024 * 1024;
        let exceeds_limit = total_bytes > limit_bytes;

        Ok(BlockstoreStats {
            total_bytes,
            file_count,
            path: self.blockstore_path.clone(),
            exists,
            exceeds_limit,
            cache_limit_mb: self.cache_limit_mb,
        })
    }

    /// Clear entire blockstore (nuclear option - removes all cached blocks)
    /// WARNING: This will require re-downloading all files
    pub fn clear_blockstore(&self) -> Result<BlockstoreCleanupReport> {
        let mut report = BlockstoreCleanupReport {
            bytes_freed: 0,
            files_deleted: 0,
            errors: Vec::new(),
            method: "clear_all".to_string(),
        };

        if !self.blockstore_path.exists() {
            return Ok(report);
        }

        // Calculate size before deletion
        let size_before = calculate_directory_size(&self.blockstore_path)?;

        // Remove the entire blockstore directory
        match fs::remove_dir_all(&self.blockstore_path) {
            Ok(_) => {
                report.bytes_freed = size_before;
                tracing::info!("Cleared blockstore at {:?}, freed {}",
                    self.blockstore_path, format_bytes(size_before));
            }
            Err(e) => {
                let err_msg = format!("Failed to remove blockstore directory: {}", e);
                tracing::error!("{}", err_msg);
                report.errors.push(err_msg);
            }
        }

        Ok(report)
    }

    /// Cleanup old blockstore files (older than specified days)
    /// This is a safer partial cleanup that preserves recent blocks
    pub fn cleanup_old_blocks(&self, max_age_days: u64) -> Result<BlockstoreCleanupReport> {
        let mut report = BlockstoreCleanupReport {
            bytes_freed: 0,
            files_deleted: 0,
            errors: Vec::new(),
            method: format!("cleanup_old_blocks (>{} days)", max_age_days),
        };

        if !self.blockstore_path.exists() {
            return Ok(report);
        }

        let max_age = std::time::Duration::from_secs(max_age_days * 86400);
        let now = std::time::SystemTime::now();

        // Recursively walk blockstore directory
        match walk_and_delete_old_files(&self.blockstore_path, max_age, now, &mut report) {
            Ok(_) => {
                tracing::info!(
                    "Cleaned up old blockstore files: {} files, {} freed",
                    report.files_deleted,
                    format_bytes(report.bytes_freed)
                );
            }
            Err(e) => {
                let err_msg = format!("Error during cleanup: {}", e);
                tracing::error!("{}", err_msg);
                report.errors.push(err_msg);
            }
        }

        Ok(report)
    }

    /// Check if blockstore needs cleanup based on size limit
    pub fn needs_cleanup(&self) -> Result<bool> {
        let stats = self.get_stats()?;
        Ok(stats.exceeds_limit)
    }

    /// Perform automatic cleanup if size exceeds limit
    /// Uses a conservative strategy: delete files older than 30 days first,
    /// then 14 days, then 7 days if still over limit
    pub fn auto_cleanup_if_needed(&self) -> Result<Option<BlockstoreCleanupReport>> {
        if !self.needs_cleanup()? {
            return Ok(None);
        }

        tracing::info!("Blockstore exceeds size limit, starting automatic cleanup");

        // Try cleaning files older than 30 days first
        let mut report = self.cleanup_old_blocks(30)?;

        // Check if we're still over limit
        if self.needs_cleanup()? {
            tracing::info!("Still over limit, cleaning files older than 14 days");
            let report_14 = self.cleanup_old_blocks(14)?;
            report.bytes_freed += report_14.bytes_freed;
            report.files_deleted += report_14.files_deleted;
            report.errors.extend(report_14.errors);
            report.method = format!("{}, {}", report.method, report_14.method);
        }

        // Last resort: files older than 7 days
        if self.needs_cleanup()? {
            tracing::info!("Still over limit, cleaning files older than 7 days");
            let report_7 = self.cleanup_old_blocks(7)?;
            report.bytes_freed += report_7.bytes_freed;
            report.files_deleted += report_7.files_deleted;
            report.errors.extend(report_7.errors);
            report.method = format!("{}, {}", report.method, report_7.method);
        }

        Ok(Some(report))
    }
}

/// Calculate total size of a directory recursively
fn calculate_directory_size(path: &Path) -> Result<u64> {
    let mut total = 0u64;

    if !path.exists() {
        return Ok(0);
    }

    if path.is_file() {
        return Ok(fs::metadata(path)?.len());
    }

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();

        if entry_path.is_file() {
            total += fs::metadata(&entry_path)?.len();
        } else if entry_path.is_dir() {
            total += calculate_directory_size(&entry_path)?;
        }
    }

    Ok(total)
}

/// Count files in a directory recursively
fn count_files(path: &Path) -> Result<usize> {
    let mut count = 0usize;

    if !path.exists() {
        return Ok(0);
    }

    if path.is_file() {
        return Ok(1);
    }

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();

        if entry_path.is_file() {
            count += 1;
        } else if entry_path.is_dir() {
            count += count_files(&entry_path)?;
        }
    }

    Ok(count)
}

/// Walk directory and delete files older than max_age
fn walk_and_delete_old_files(
    path: &Path,
    max_age: std::time::Duration,
    now: std::time::SystemTime,
    report: &mut BlockstoreCleanupReport,
) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();

        if entry_path.is_file() {
            // Check file age
            if let Ok(metadata) = fs::metadata(&entry_path) {
                if let Ok(modified) = metadata.modified() {
                    if let Ok(age) = now.duration_since(modified) {
                        if age > max_age {
                            // File is old enough to delete
                            let size = metadata.len();

                            match fs::remove_file(&entry_path) {
                                Ok(_) => {
                                    report.bytes_freed += size;
                                    report.files_deleted += 1;
                                    tracing::debug!("Deleted old blockstore file: {:?}", entry_path);
                                }
                                Err(e) => {
                                    let err_msg = format!("Failed to delete {:?}: {}", entry_path, e);
                                    tracing::warn!("{}", err_msg);
                                    report.errors.push(err_msg);
                                }
                            }
                        }
                    }
                }
            }
        } else if entry_path.is_dir() {
            // Recursively process subdirectory
            walk_and_delete_old_files(&entry_path, max_age, now, report)?;

            // Try to remove empty directories
            if let Ok(mut entries) = fs::read_dir(&entry_path) {
                if entries.next().is_none() {
                    // Directory is empty, try to remove it
                    let _ = fs::remove_dir(&entry_path);
                }
            }
        }
    }

    Ok(())
}

/// Format bytes to human-readable string
fn format_bytes(bytes: u64) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.00 MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.00 GB");
    }

    #[test]
    fn test_blockstore_stats() {
        let stats = BlockstoreStats {
            total_bytes: 500 * 1024 * 1024, // 500 MB
            file_count: 100,
            path: PathBuf::from("/tmp/test"),
            exists: true,
            exceeds_limit: false,
            cache_limit_mb: 1024, // 1 GB
        };

        assert_eq!(stats.usage_percentage(), 48.828125); // ~48.8%
        assert!(!stats.exceeds_limit);
    }

    #[test]
    fn test_blockstore_exceeds_limit() {
        let stats = BlockstoreStats {
            total_bytes: 2 * 1024 * 1024 * 1024, // 2 GB
            file_count: 1000,
            path: PathBuf::from("/tmp/test"),
            exists: true,
            exceeds_limit: true,
            cache_limit_mb: 1024, // 1 GB
        };

        assert_eq!(stats.usage_percentage(), 200.0);
        assert!(stats.exceeds_limit);
    }
}
