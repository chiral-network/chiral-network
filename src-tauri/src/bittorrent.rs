//! BitTorrent protocol handling with seeding and event support.
//!
//! Implementation for BitTorrent functionality using librqbit
//! for downloading and seeding files with progress tracking.

use anyhow::{Result, anyhow};
use std::path::Path;
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::{broadcast, RwLock};
use tracing::{info, instrument, error, warn};
use serde::{Deserialize, Serialize};
use crate::config::bittorrent::BitTorrentConfig;

/// Events emitted during BitTorrent operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TorrentEvent {
    /// Progress update for a torrent
    Progress {
        info_hash: String,
        downloaded: u64,
        total: u64,
        speed: f64, // bytes per second
        peers: usize,
        eta_seconds: Option<u64>,
    },
    /// Torrent download completed
    Complete {
        info_hash: String,
        name: String,
        path: String,
        total_bytes: u64,
    },
    /// Torrent seeding started
    SeedingStarted {
        info_hash: String,
        magnet_link: String,
        path: String,
        name: String,
    },
    /// Error occurred during torrent operation
    Error {
        info_hash: Option<String>,
        message: String,
        error_type: String,
    },
    /// Torrent paused
    Paused {
        info_hash: String,
    },
    /// Torrent resumed
    Resumed {
        info_hash: String,
    },
    /// Torrent added to session
    Added {
        info_hash: String,
        name: String,
    },
    /// Torrent removed from session
    Removed {
        info_hash: String,
    },
}

/// A trait for handling BitTorrent operations like downloading and seeding.
#[async_trait::async_trait]
pub trait TorrentHandler {
    /// Downloads a torrent from a magnet link or torrent file.
    ///
    /// # Arguments
    ///
    /// * `torrent_source` - A string representing the magnet link or path to a .torrent file.
    /// * `download_path` - The path where the downloaded content should be saved.
    async fn download(&self, torrent_source: &str, download_path: &Path) -> Result<String>;

    /// Creates a torrent for a given file or directory and starts seeding it.
    ///
    /// # Arguments
    ///
    /// * `content_path` - The path to the file or directory to be seeded.
    /// * `announce_urls` - Optional list of tracker URLs
    /// 
    /// # Returns
    /// 
    /// The magnet link for the created torrent
    async fn seed(&self, content_path: &Path, announce_urls: Option<Vec<String>>) -> Result<String>;

    /// Subscribe to torrent events
    fn subscribe_events(&self) -> broadcast::Receiver<TorrentEvent>;

    /// Pause a torrent by info hash
    async fn pause_torrent(&self, info_hash: &str) -> Result<()>;

    /// Resume a torrent by info hash
    async fn resume_torrent(&self, info_hash: &str) -> Result<()>;

    /// Remove a torrent from the session
    async fn remove_torrent(&self, info_hash: &str, delete_files: bool) -> Result<()>;

    /// Get status of all active torrents
    async fn get_torrent_status(&self) -> Result<Vec<TorrentStatus>>;

    /// Get detailed statistics for a specific torrent
    async fn get_torrent_details(&self, info_hash: &str) -> Result<TorrentDetails>;
}

/// Status information for a torrent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentStatus {
    pub info_hash: String,
    pub name: String,
    pub downloaded: u64,
    pub total: u64,
    pub progress: f64,
    pub download_speed: f64,
    pub upload_speed: f64,
    pub peers_connected: usize,
    pub peers_total: usize,
    pub state: TorrentState,
    pub eta_seconds: Option<u64>,
    pub ratio: f64,
}

/// Detailed information for a torrent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentDetails {
    pub info_hash: String,
    pub name: String,
    pub downloaded: u64,
    pub uploaded: u64,
    pub total: u64,
    pub download_speed: f64,
    pub upload_speed: f64,
    pub peers: Vec<PeerInfo>,
    pub files: Vec<FileInfo>,
    pub trackers: Vec<TrackerInfo>,
    pub state: TorrentState,
    pub created_date: Option<String>,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub ip: String,
    pub port: u16,
    pub client: Option<String>,
    pub downloaded: u64,
    pub uploaded: u64,
    pub progress: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: String,
    pub size: u64,
    pub downloaded: u64,
    pub progress: f64,
    pub priority: FilePriority,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackerInfo {
    pub url: String,
    pub status: TrackerStatus,
    pub peers: usize,
    pub last_announce: Option<String>,
    pub next_announce: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TorrentState {
    Downloading,
    Seeding,
    Paused,
    Error,
    Complete,
    Checking,
    QueuedForDownload,
    QueuedForSeeding,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FilePriority {
    Skip,
    Low,
    Normal,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TrackerStatus {
    Working,
    Updating,
    NotWorking,
    NotContacted,
}

/// Internal torrent tracking information
#[derive(Debug, Clone)]
struct TorrentInfo {
    info_hash: String,
    name: String,
    path: String,
    state: TorrentState,
    added_time: std::time::SystemTime,
}

/// A handler for BitTorrent operations using librqbit.
pub struct BitTorrentHandler {
    event_sender: broadcast::Sender<TorrentEvent>,
    config: Arc<RwLock<BitTorrentConfig>>,
    torrents: Arc<RwLock<HashMap<String, TorrentInfo>>>,
    // TODO: Add librqbit session here when implementing
    // session: Option<Arc<librqbit::Session>>,
}

impl BitTorrentHandler {
    /// Creates a new `BitTorrentHandler` with configuration.
    pub fn new(config: BitTorrentConfig) -> Self {
        info!("BitTorrentHandler initialized with config");
        let (event_sender, _) = broadcast::channel(1000);
        
        Self {
            event_sender,
            config: Arc::new(RwLock::new(config)),
            torrents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Initialize the BitTorrent session with current configuration
    pub async fn initialize(&mut self) -> Result<()> {
        let config = self.config.read().await;
        
        // TODO: Initialize librqbit session here
        // let session_config = self.create_session_config(&config).await?;
        // self.session = Some(Arc::new(librqbit::Session::new(session_config).await?));
        
        info!("BitTorrent session initialized");
        Ok(())
    }

    /// Update configuration and restart session if needed
    pub async fn update_config(&self, new_config: BitTorrentConfig) -> Result<()> {
        let mut config = self.config.write().await;
        *config = new_config;
        
        // TODO: Update session configuration
        // if let Some(session) = &self.session {
        //     session.update_config(&*config).await?;
        // }
        
        info!("BitTorrentHandler configuration updated");
        Ok(())
    }

    /// Get current configuration
    pub async fn get_config(&self) -> BitTorrentConfig {
        let config = self.config.read().await;
        config.clone()
    }

    /// Send an event to all subscribers
    fn emit_event(&self, event: TorrentEvent) {
        if let Err(e) = self.event_sender.send(event) {
            warn!("Failed to send torrent event: {}", e);
        }
    }

    /// Generate magnet link from torrent info
    fn generate_magnet_link(&self, info_hash: &str, name: &str, announce_urls: &[String]) -> String {
        let mut magnet = format!("magnet:?xt=urn:btih:{}&dn={}", info_hash, urlencoding::encode(name));
        
        for url in announce_urls {
            magnet.push_str(&format!("&tr={}", urlencoding::encode(url)));
        }
        
        magnet
    }

    /// Create torrent file from content using librqbit
    async fn create_torrent_from_path(&self, content_path: &Path, announce_urls: Option<Vec<String>>) -> Result<(String, String, String)> {
        let name = content_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        // TODO: Replace with actual librqbit implementation
        // For now, simulate torrent creation
        let info_hash = format!("{:x}", md5::compute(content_path.to_string_lossy().as_bytes()));
        
        let config = self.config.read().await;
        let trackers = announce_urls.unwrap_or_else(|| {
            config.dht.bootstrap_nodes.clone()
        });

        let magnet_link = self.generate_magnet_link(&info_hash, &name, &trackers);

        // TODO: Actual implementation would be:
        // let torrent_builder = librqbit::TorrentBuilder::new()
        //     .set_name(&name)
        //     .set_announce_list(trackers)
        //     .add_file_or_directory(content_path)?;
        // 
        // let torrent_info = torrent_builder.build().await?;
        // let info_hash = torrent_info.info_hash().to_string();
        // let magnet_link = torrent_info.to_magnet_link();

        Ok((info_hash, magnet_link, name))
    }

    /// Start monitoring torrent progress
    async fn monitor_torrent_progress(&self, info_hash: String, name: String) {
        let sender = self.event_sender.clone();
        let torrents = self.torrents.clone();

        tokio::spawn(async move {
            // TODO: Replace with actual librqbit progress monitoring
            // For now, simulate progress updates
            let mut downloaded = 0u64;
            let total = 1024 * 1024 * 100; // 100MB simulated file size
            let mut speed = 1024.0 * 512.0; // 512 KB/s initial speed

            while downloaded < total {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                
                let increment = (speed as u64).min(total - downloaded);
                downloaded += increment;
                speed *= 0.95; // Simulate decreasing speed

                let eta_seconds = if speed > 0.0 {
                    Some(((total - downloaded) as f64 / speed) as u64)
                } else {
                    None
                };

                let _ = sender.send(TorrentEvent::Progress {
                    info_hash: info_hash.clone(),
                    downloaded,
                    total,
                    speed,
                    peers: 5, // Simulated peer count
                    eta_seconds,
                });

                if downloaded >= total {
                    // Update torrent state
                    if let Ok(mut torrents_map) = torrents.write() {
                        if let Some(torrent) = torrents_map.get_mut(&info_hash) {
                            torrent.state = TorrentState::Complete;
                        }
                    }

                    let _ = sender.send(TorrentEvent::Complete {
                        info_hash: info_hash.clone(),
                        name: name.clone(),
                        path: "/simulated/path".to_string(),
                        total_bytes: total,
                    });
                    break;
                }
            }
        });
    }

    /// Add torrent to internal tracking
    async fn add_torrent_info(&self, info_hash: String, name: String, path: String, state: TorrentState) {
        let mut torrents = self.torrents.write().await;
        torrents.insert(info_hash.clone(), TorrentInfo {
            info_hash,
            name,
            path,
            state,
            added_time: std::time::SystemTime::now(),
        });
    }

    /// Remove torrent from internal tracking
    async fn remove_torrent_info(&self, info_hash: &str) {
        let mut torrents = self.torrents.write().await;
        torrents.remove(info_hash);
    }

    /// Update torrent state in internal tracking
    async fn update_torrent_state(&self, info_hash: &str, state: TorrentState) {
        let mut torrents = self.torrents.write().await;
        if let Some(torrent) = torrents.get_mut(info_hash) {
            torrent.state = state;
        }
    }
}

impl Default for BitTorrentHandler {
    fn default() -> Self {
        Self::new(BitTorrentConfig::default())
    }
}

#[async_trait::async_trait]
impl TorrentHandler for BitTorrentHandler {
    #[instrument(skip(self), fields(source = %torrent_source, path = %download_path.display()))]
    async fn download(&self, torrent_source: &str, download_path: &Path) -> Result<String> {
        info!("Starting torrent download");
        
        // Extract or generate info hash
        let info_hash = if torrent_source.starts_with("magnet:") {
            // Parse magnet link to extract info hash
            if let Some(xt_pos) = torrent_source.find("xt=urn:btih:") {
                let start = xt_pos + 13;
                let end = torrent_source[start..].find('&').map(|i| start + i).unwrap_or(torrent_source.len());
                torrent_source[start..end].to_string()
            } else {
                return Err(anyhow!("Invalid magnet link format"));
            }
        } else {
            // For .torrent files, we'd parse the file to get the info hash
            format!("{:x}", md5::compute(torrent_source.as_bytes()))
        };

        // TODO: Replace with actual librqbit implementation
        // let session = self.session.as_ref().ok_or_else(|| anyhow!("Session not initialized"))?;
        // let torrent = session.add_torrent(torrent_source, download_path).await?;
        // let info_hash = torrent.info_hash().to_string();

        let name = "Simulated Torrent"; // This would come from torrent metadata
        
        // Add to tracking
        self.add_torrent_info(
            info_hash.clone(),
            name.to_string(),
            download_path.to_string_lossy().to_string(),
            TorrentState::Downloading,
        ).await;

        // Emit added event
        self.emit_event(TorrentEvent::Added {
            info_hash: info_hash.clone(),
            name: name.to_string(),
        });

        // Start progress monitoring
        self.monitor_torrent_progress(info_hash.clone(), name.to_string()).await;

        info!("Started download of torrent: {}", info_hash);
        Ok(info_hash)
    }

    #[instrument(skip(self), fields(path = %content_path.display()))]
    async fn seed(&self, content_path: &Path, announce_urls: Option<Vec<String>>) -> Result<String> {
        info!("Starting to seed content");
        
        if !content_path.exists() {
            let error_msg = format!("Content path does not exist: {}", content_path.display());
            self.emit_event(TorrentEvent::Error {
                info_hash: None,
                message: error_msg.clone(),
                error_type: "FileNotFound".to_string(),
            });
            return Err(anyhow!(error_msg));
        }

        // Create torrent from the content
        let (info_hash, magnet_link, name) = self.create_torrent_from_path(content_path, announce_urls).await?;

        // TODO: Implement actual seeding with librqbit
        // let session = self.session.as_ref().ok_or_else(|| anyhow!("Session not initialized"))?;
        // let torrent = session.create_torrent_and_seed(content_path, &announce_urls).await?;
        
        // Add to tracking
        self.add_torrent_info(
            info_hash.clone(),
            name.clone(),
            content_path.to_string_lossy().to_string(),
            TorrentState::Seeding,
        ).await;

        self.emit_event(TorrentEvent::SeedingStarted {
            info_hash: info_hash.clone(),
            magnet_link: magnet_link.clone(),
            path: content_path.to_string_lossy().to_string(),
            name: name.clone(),
        });

        info!("Started seeding: {} with magnet: {}", name, magnet_link);
        Ok(magnet_link)
    }

    fn subscribe_events(&self) -> broadcast::Receiver<TorrentEvent> {
        self.event_sender.subscribe()
    }

    async fn pause_torrent(&self, info_hash: &str) -> Result<()> {
        // TODO: Implement actual pause with librqbit
        // let session = self.session.as_ref().ok_or_else(|| anyhow!("Session not initialized"))?;
        // session.pause_torrent(info_hash).await?;

        self.update_torrent_state(info_hash, TorrentState::Paused).await;
        
        self.emit_event(TorrentEvent::Paused {
            info_hash: info_hash.to_string(),
        });
        
        info!("Paused torrent: {}", info_hash);
        Ok(())
    }

    async fn resume_torrent(&self, info_hash: &str) -> Result<()> {
        // TODO: Implement actual resume with librqbit
        // let session = self.session.as_ref().ok_or_else(|| anyhow!("Session not initialized"))?;
        // session.resume_torrent(info_hash).await?;

        self.update_torrent_state(info_hash, TorrentState::Downloading).await;
        
        self.emit_event(TorrentEvent::Resumed {
            info_hash: info_hash.to_string(),
        });
        
        info!("Resumed torrent: {}", info_hash);
        Ok(())
    }

    async fn remove_torrent(&self, info_hash: &str, delete_files: bool) -> Result<()> {
        // TODO: Implement actual removal with librqbit
        // let session = self.session.as_ref().ok_or_else(|| anyhow!("Session not initialized"))?;
        // session.remove_torrent(info_hash, delete_files).await?;

        self.remove_torrent_info(info_hash).await;
        
        self.emit_event(TorrentEvent::Removed {
            info_hash: info_hash.to_string(),
        });
        
        info!("Removed torrent: {} (delete_files: {})", info_hash, delete_files);
        Ok(())
    }

    async fn get_torrent_status(&self) -> Result<Vec<TorrentStatus>> {
        // TODO: Implement actual status retrieval with librqbit
        let torrents = self.torrents.read().await;
        let mut status_list = Vec::new();

        for (info_hash, torrent_info) in torrents.iter() {
            // TODO: Get real stats from librqbit session
            let status = TorrentStatus {
                info_hash: info_hash.clone(),
                name: torrent_info.name.clone(),
                downloaded: 50 * 1024 * 1024, // Simulated
                total: 100 * 1024 * 1024,     // Simulated
                progress: 0.5,                // Simulated
                download_speed: 1024.0 * 256.0, // Simulated
                upload_speed: 1024.0 * 64.0,    // Simulated
                peers_connected: 5,           // Simulated
                peers_total: 10,              // Simulated
                state: torrent_info.state.clone(),
                eta_seconds: Some(300),       // Simulated
                ratio: 1.5,                   // Simulated
            };
            status_list.push(status);
        }

        Ok(status_list)
    }

    async fn get_torrent_details(&self, info_hash: &str) -> Result<TorrentDetails> {
        // TODO: Implement actual details retrieval with librqbit
        let torrents = self.torrents.read().await;
        
        if let Some(torrent_info) = torrents.get(info_hash) {
            let details = TorrentDetails {
                info_hash: info_hash.to_string(),
                name: torrent_info.name.clone(),
                downloaded: 50 * 1024 * 1024,
                uploaded: 75 * 1024 * 1024,
                total: 100 * 1024 * 1024,
                download_speed: 1024.0 * 256.0,
                upload_speed: 1024.0 * 64.0,
                peers: vec![], // TODO: Get from librqbit
                files: vec![], // TODO: Get from librqbit
                trackers: vec![], // TODO: Get from librqbit
                state: torrent_info.state.clone(),
                created_date: None,
                comment: None,
            };
            Ok(details)
        } else {
            Err(anyhow!("Torrent not found: {}", info_hash))
        }
    }
}

// Tauri commands for frontend integration
#[tauri::command]
pub async fn download_torrent(
    handler: tauri::State<'_, Arc<RwLock<BitTorrentHandler>>>,
    torrent_source: String,
    download_path: String,
) -> Result<String, String> {
    let handler = handler.read().await;
    handler.download(&torrent_source, Path::new(&download_path))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn seed_content(
    handler: tauri::State<'_, Arc<RwLock<BitTorrentHandler>>>,
    content_path: String,
    announce_urls: Option<Vec<String>>,
) -> Result<String, String> {
    let handler = handler.read().await;
    handler.seed(Path::new(&content_path), announce_urls)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn pause_torrent(
    handler: tauri::State<'_, Arc<RwLock<BitTorrentHandler>>>,
    info_hash: String,
) -> Result<(), String> {
    let handler = handler.read().await;
    handler.pause_torrent(&info_hash)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn resume_torrent(
    handler: tauri::State<'_, Arc<RwLock<BitTorrentHandler>>>,
    info_hash: String,
) -> Result<(), String> {
    let handler = handler.read().await;
    handler.resume_torrent(&info_hash)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remove_torrent(
    handler: tauri::State<'_, Arc<RwLock<BitTorrentHandler>>>,
    info_hash: String,
    delete_files: bool,
) -> Result<(), String> {
    let handler = handler.read().await;
    handler.remove_torrent(&info_hash, delete_files)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_torrent_status(
    handler: tauri::State<'_, Arc<RwLock<BitTorrentHandler>>>,
) -> Result<Vec<TorrentStatus>, String> {
    let handler = handler.read().await;
    handler.get_torrent_status()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_torrent_details(
    handler: tauri::State<'_, Arc<RwLock<BitTorrentHandler>>>,
    info_hash: String,
) -> Result<TorrentDetails, String> {
    let handler = handler.read().await;
    handler.get_torrent_details(&info_hash)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn torrent_download(
    handler: tauri::State<'_, Arc<RwLock<BitTorrentHandler>>>,
    identifier: String,
    download_path: String,
) -> Result<(), String> {
    let handler = handler.read().await;
    handler
        .download(&identifier, Path::new(&download_path))
        .await
        .map(|_| ()) // Discard the info_hash to match Result<(), String>
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn torrent_seed(
    handler: tauri::State<'_, Arc<RwLock<BitTorrentHandler>>>,
    file_path: String,
    announce_urls: Option<Vec<String>>,
) -> Result<String, String> {
    let handler = handler.read().await;
    handler
        .seed(Path::new(&file_path), announce_urls)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn torrent_get_active(
    handler: tauri::State<'_, Arc<RwLock<BitTorrentHandler>>>,
) -> Result<Vec<TorrentStatus>, String> {
    let handler = handler.read().await;
    handler.get_torrent_status().await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dht::DhtService;
    use std::time::Duration;
    use tokio::time::timeout;

    /// Helper to create a DHT service for testing.
    async fn create_test_dht_node(port: u16, bootstrap_nodes: Vec<String>) -> DhtService {
        DhtService::new(
            port,
            bootstrap_nodes,
            None,      // secret
            false,     // is_bootstrap
            false,     // enable_autonat
            None,      // autonat_probe_interval
            vec![],    // autonat_servers
            None,      // proxy_address
            None,      // file_transfer_service
            None,      // chunk_manager
            Some(256), // chunk_size_kb
            Some(128), // cache_size_mb
            false,     // enable_autorelay
            vec![],    // preferred_relays
            false,     // enable_relay_server
            None,      // blockstore_db_path
        )
        .await
        .expect("Failed to create DHT service")
    }

    #[tokio::test]
    async fn test_event_system() {
        let handler = BitTorrentHandler::new(BitTorrentConfig::default());
        let mut receiver = handler.subscribe_events();

        // Test event emission
        handler.emit_event(TorrentEvent::Progress {
            info_hash: "test_hash".to_string(),
            downloaded: 100,
            total: 1000,
            speed: 50.0,
            peers: 3,
            eta_seconds: Some(180),
        });

        // Verify event received
        let event = timeout(Duration::from_secs(1), receiver.recv()).await
            .expect("Timeout waiting for event")
            .expect("Failed to receive event");

        match event {
            TorrentEvent::Progress { info_hash, downloaded, total, speed, peers, eta_seconds } => {
                assert_eq!(info_hash, "test_hash");
                assert_eq!(downloaded, 100);
                assert_eq!(total, 1000);
                assert_eq!(speed, 50.0);
                assert_eq!(peers, 3);
                assert_eq!(eta_seconds, Some(180));
            }
            _ => panic!("Unexpected event type"),
        }
    }

    #[tokio::test]
    async fn test_seeding() {
        let handler = BitTorrentHandler::new(BitTorrentConfig::default());
        let temp_file = std::env::temp_dir().join("test_seed_file.txt");
        
        // Create a test file
        std::fs::write(&temp_file, "test content").expect("Failed to create test file");

        let result = handler.seed(&temp_file, None).await;
        assert!(result.is_ok());

        let magnet_link = result.unwrap();
        assert!(magnet_link.starts_with("magnet:?xt=urn:btih:"));

        // Cleanup
        let _ = std::fs::remove_file(temp_file);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_dht_torrent_peer_discovery() {
        // 1. Setup: Create two DHT nodes.
        let node1 = create_test_dht_node(10001, vec![]).await;
        let node1_peer_id = node1.get_peer_id().await;
        let node1_addr = format!("/ip4/127.0.0.1/tcp/10001/p2p/{}", node1_peer_id);

        let node2 = create_test_dht_node(10002, vec![node1_addr.clone()]).await;
        let node2_peer_id = node2.get_peer_id().await;

        // Give nodes time to connect
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Ensure they are connected
        assert!(node1.get_connected_peers().await.contains(&node2_peer_id));
        assert!(node2.get_connected_peers().await.contains(&node1_peer_id));

        // 2. Announce: Node 1 announces it's seeding a torrent.
        let info_hash = "b263275b1e3138b29596356533f685c33103575c".to_string();
        node1
            .announce_torrent(info_hash.clone())
            .await
            .expect("Node 1 failed to announce torrent");

        // Give DHT time to propagate provider record
        tokio::time::sleep(Duration::from_secs(3)).await;

        // 3. Discover: Node 2 searches for peers seeding that torrent.
        let providers = node2.get_seeders_for_file(&info_hash).await;

        // 4. Assert: Node 2 should find Node 1.
        assert!(!providers.is_empty(), "Node 2 should have found providers.");
        assert!(providers.contains(&node1_peer_id), "Node 2 did not discover Node 1 as a provider for the torrent.");
    }

    #[tokio::test]
    async fn test_torrent_lifecycle() {
        let mut handler = BitTorrentHandler::new(BitTorrentConfig::default());
        handler.initialize().await.expect("Failed to initialize handler");

        let temp_file = std::env::temp_dir().join("test_lifecycle_file.txt");
        std::fs::write(&temp_file, "test content for lifecycle").expect("Failed to create test file");

        // Test seeding
        let magnet_link = handler.seed(&temp_file, None).await.expect("Failed to seed");
        assert!(magnet_link.starts_with("magnet:"));

        // Test getting status
        let status = handler.get_torrent_status().await.expect("Failed to get status");
        assert_eq!(status.len(), 1);
        
        let torrent_status = &status[0];
        assert_eq!(torrent_status.state, TorrentState::Seeding);

        // Test pause/resume
        handler.pause_torrent(&torrent_status.info_hash).await.expect("Failed to pause");
        handler.resume_torrent(&torrent_status.info_hash).await.expect("Failed to resume");

        // Test removal
        handler.remove_torrent(&torrent_status.info_hash, false).await.expect("Failed to remove");
        
        let status_after_removal = handler.get_torrent_status().await.expect("Failed to get status");
        assert_eq!(status_after_removal.len(), 0);

        // Cleanup
        let _ = std::fs::remove_file(temp_file);
    }
}