// download_scheduler.rs
// Example integration of unified download source abstraction
// This module demonstrates how to use DownloadSource in scheduling and logging

use crate::download_source::{DownloadSource, FtpSourceInfo, HttpSourceInfo, P2pSourceInfo};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info, warn};

/// Represents a scheduled download task
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadTask {
    /// Unique task identifier
    pub task_id: String,

    /// File hash or identifier
    pub file_hash: String,

    /// File name
    pub file_name: String,

    /// Available download sources
    pub sources: Vec<DownloadSource>,

    /// Task status
    pub status: DownloadTaskStatus,

    /// Priority (higher is more important)
    pub priority: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadTaskStatus {
    Pending,
    Scheduled,
    Downloading,
    Paused,
    Completed,
    Failed,
}

/// Download scheduler that manages tasks with different source types
pub struct DownloadScheduler {
    tasks: HashMap<String, DownloadTask>,
}

impl DownloadScheduler {
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
        }
    }

    /// Add a new download task with multiple sources
    pub fn add_task(&mut self, task: DownloadTask) {
        info!(
            task_id = %task.task_id,
            file_hash = %task.file_hash,
            sources_count = task.sources.len(),
            "Adding new download task"
        );

        // Log information about each source
        for (idx, source) in task.sources.iter().enumerate() {
            debug!(
                task_id = %task.task_id,
                source_idx = idx,
                source_type = source.source_type(),
                source_display = %source,
                supports_encryption = source.supports_encryption(),
                priority_score = source.priority_score(),
                "Source available for download"
            );
        }

        self.tasks.insert(task.task_id.clone(), task);
    }

    /// Select the best source for a download task
    pub fn select_best_source(&self, task_id: &str) -> Option<DownloadSource> {
        let task = self.tasks.get(task_id)?;

        if task.sources.is_empty() {
            warn!(task_id = %task_id, "No sources available for task");
            return None;
        }

        // Sort sources by priority score (highest first)
        let mut sources_with_scores: Vec<_> = task
            .sources
            .iter()
            .map(|s| (s.clone(), s.priority_score()))
            .collect();

        sources_with_scores.sort_by(|a, b| b.1.cmp(&a.1));

        let best_source = sources_with_scores[0].0.clone();

        info!(
            task_id = %task_id,
            source_type = best_source.source_type(),
            source = %best_source,
            priority_score = best_source.priority_score(),
            "Selected best source for download"
        );

        Some(best_source)
    }

    /// Handle source-specific download logic (placeholder)
    pub fn start_download(&self, task_id: &str, source: &DownloadSource) -> Result<(), String> {
        info!(
            task_id = %task_id,
            source_type = source.source_type(),
            "Starting download from source"
        );

        match source {
            DownloadSource::P2p(info) => {
                self.handle_p2p_download(task_id, info)
            }
            DownloadSource::Http(info) => {
                self.handle_http_download(task_id, info)
            }
            DownloadSource::Ftp(info) => {
                self.handle_ftp_download(task_id, info)
            }
        }
    }

    // Placeholder handlers for different source types
    fn handle_p2p_download(&self, task_id: &str, info: &P2pSourceInfo) -> Result<(), String> {
        info!(
            task_id = %task_id,
            peer_id = %info.peer_id,
            protocol = ?info.protocol,
            "Initiating P2P download"
        );
        // TODO: Implement actual P2P download logic
        Ok(())
    }

    fn handle_http_download(&self, task_id: &str, info: &HttpSourceInfo) -> Result<(), String> {
        info!(
            task_id = %task_id,
            url = %info.url,
            verify_ssl = info.verify_ssl,
            "Initiating HTTP download"
        );
        // TODO: Implement actual HTTP download logic
        Ok(())
    }

    fn handle_ftp_download(&self, task_id: &str, info: &FtpSourceInfo) -> Result<(), String> {
        info!(
            task_id = %task_id,
            url = %info.url,
            username = ?info.username,
            passive_mode = info.passive_mode,
            use_ftps = info.use_ftps,
            "Initiating FTP download"
        );
        // TODO: Implement actual FTP download logic
        // This is where you would:
        // 1. Parse the FTP URL
        // 2. Connect to the FTP server
        // 3. Authenticate if credentials provided
        // 4. Download the file
        // 5. Handle passive/active mode
        // 6. Handle FTPS if enabled
        Ok(())
    }

    /// Get statistics about source types in use
    pub fn get_source_statistics(&self) -> SourceStatistics {
        let mut stats = SourceStatistics::default();

        for task in self.tasks.values() {
            for source in &task.sources {
                match source {
                    DownloadSource::P2p(_) => stats.p2p_count += 1,
                    DownloadSource::Http(_) => stats.http_count += 1,
                    DownloadSource::Ftp(_) => stats.ftp_count += 1,
                }
            }
        }

        info!(
            p2p_sources = stats.p2p_count,
            http_sources = stats.http_count,
            ftp_sources = stats.ftp_count,
            "Current source statistics"
        );

        stats
    }

    /// Display all tasks with their sources
    pub fn display_tasks(&self) {
        info!(total_tasks = self.tasks.len(), "Current download tasks");

        for task in self.tasks.values() {
            info!(
                task_id = %task.task_id,
                file_name = %task.file_name,
                status = ?task.status,
                priority = task.priority,
                sources_count = task.sources.len(),
                "Task details"
            );

            for source in &task.sources {
                debug!(
                    task_id = %task.task_id,
                    source = %source,
                    "  └─ Source available"
                );
            }
        }
    }
}

impl Default for DownloadScheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Default, Serialize)]
pub struct SourceStatistics {
    pub p2p_count: usize,
    pub http_count: usize,
    pub ftp_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_with_mixed_sources() {
        let mut scheduler = DownloadScheduler::new();

        let task = DownloadTask {
            task_id: "task1".to_string(),
            file_hash: "QmTest123".to_string(),
            file_name: "test_file.zip".to_string(),
            sources: vec![
                DownloadSource::P2p(P2pSourceInfo {
                    peer_id: "12D3KooWPeer1".to_string(),
                    multiaddr: None,
                    reputation: Some(90),
                    supports_encryption: true,
                    protocol: Some("webrtc".to_string()),
                }),
                DownloadSource::Http(HttpSourceInfo {
                    url: "https://cdn.example.com/file.zip".to_string(),
                    auth_header: None,
                    verify_ssl: true,
                    headers: None,
                    timeout_secs: Some(30),
                }),
                DownloadSource::Ftp(FtpSourceInfo {
                    url: "ftp://ftp.example.com/pub/file.zip".to_string(),
                    username: Some("anonymous".to_string()),
                    encrypted_password: None,
                    passive_mode: true,
                    use_ftps: false,
                    timeout_secs: Some(60),
                }),
            ],
            status: DownloadTaskStatus::Pending,
            priority: 100,
        };

        scheduler.add_task(task);

        // Should select P2P as best source (highest priority)
        let best_source = scheduler.select_best_source("task1").unwrap();
        assert_eq!(best_source.source_type(), "P2P");

        // Check statistics
        let stats = scheduler.get_source_statistics();
        assert_eq!(stats.p2p_count, 1);
        assert_eq!(stats.http_count, 1);
        assert_eq!(stats.ftp_count, 1);
    }

    #[test]
    fn test_ftp_source_recognition() {
        let ftp_source = DownloadSource::Ftp(FtpSourceInfo {
            url: "ftp://files.example.org/data.tar.gz".to_string(),
            username: Some("user".to_string()),
            encrypted_password: Some("encrypted_pass_base64".to_string()),
            passive_mode: true,
            use_ftps: true,
            timeout_secs: Some(120),
        });

        assert_eq!(ftp_source.source_type(), "FTP");
        assert_eq!(ftp_source.display_name(), "FTP: files.example.org");
        assert!(ftp_source.supports_encryption()); // FTPS enabled
        assert_eq!(ftp_source.priority_score(), 25);
    }
}
