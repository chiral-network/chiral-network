use libp2p::gossipsub::IdentTopic;
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tokio::time::{timeout, Duration};
use tracing::{debug, info, warn};

use crate::dht::models::FileMetadata;
use crate::encryption::EncryptedAesKeyBundle;

/// General seeder info (topic: seeder/{peerID})
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeederGeneralInfo {
    #[serde(rename = "peerId")]
    pub peer_id: String,
    #[serde(rename = "walletAddress")]
    pub wallet_address: String,
    #[serde(rename = "defaultPricePerMb")]
    pub default_price_per_mb: f64,
    pub timestamp: u64,
}

/// File-specific info (topic: seeder/{peerID}/file/{fileHash})
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeederFileInfo {
    #[serde(rename = "peerId")]
    pub peer_id: String,
    #[serde(rename = "fileHash")]
    pub file_hash: String,
    #[serde(rename = "pricePerMb")]
    pub price_per_mb: Option<f64>, // Overrides default if set
    #[serde(rename = "supportedProtocols")]
    pub supported_protocols: Vec<String>,
    #[serde(rename = "protocolDetails")]
    pub protocol_details: ProtocolDetails,
    pub timestamp: u64,
}

/// Protocol-specific details, grouped by protocol type
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProtocolDetails {
    // Protocol-specific details (only populated for supported protocols)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http: Option<HttpProtocolDetails>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub ftp: Option<FtpProtocolDetails>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub ed2k: Option<Ed2kProtocolDetails>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bittorrent: Option<BitTorrentProtocolDetails>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bitswap: Option<BitswapProtocolDetails>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub webrtc: Option<WebRtcProtocolDetails>,

    // Common encryption (applies to all protocols)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption: Option<EncryptionDetails>,
}

/// HTTP Protocol Details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpProtocolDetails {
    pub sources: Vec<HttpSourceInfo>,
}

/// FTP Protocol Details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FtpProtocolDetails {
    pub sources: Vec<FtpSourceInfo>,
}

/// ED2K Protocol Details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ed2kProtocolDetails {
    pub sources: Vec<Ed2kSourceInfo>,
}

/// BitTorrent Protocol Details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitTorrentProtocolDetails {
    #[serde(rename = "infoHash")]
    pub info_hash: String,
    pub trackers: Vec<String>,
}

/// BitSwap/IPFS Protocol Details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitswapProtocolDetails {
    pub cids: Vec<String>,
    #[serde(rename = "isRoot")]
    pub is_root: bool,
}

/// WebRTC Protocol Details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebRtcProtocolDetails {
    pub enabled: bool,
}

/// Common Encryption Details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionDetails {
    pub method: String,
    #[serde(rename = "keyFingerprint")]
    pub key_fingerprint: String,
    #[serde(rename = "encryptedKeyBundle")]
    pub encrypted_key_bundle: EncryptedAesKeyBundle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpSourceInfo {
    pub url: String,
    #[serde(rename = "authHeader")]
    pub auth_header: Option<String>,
    #[serde(rename = "verifySsl")]
    pub verify_ssl: bool,
    pub headers: Option<Vec<(String, String)>>,
    #[serde(rename = "timeoutSecs")]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FtpSourceInfo {
    pub url: String,
    pub username: Option<String>,
    #[serde(rename = "encryptedPassword")]
    pub encrypted_password: Option<String>, // Encrypted with file hash as key
    #[serde(rename = "passiveMode")]
    pub passive_mode: bool,
    #[serde(rename = "useFtps")]
    pub use_ftps: bool,
    #[serde(rename = "timeoutSecs")]
    pub timeout_secs: Option<u64>,
    #[serde(rename = "supportsResume")]
    pub supports_resume: bool,
    #[serde(rename = "fileSize")]
    pub file_size: u64,
    #[serde(rename = "lastChecked")]
    pub last_checked: Option<u64>,
    #[serde(rename = "isAvailable")]
    pub is_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ed2kSourceInfo {
    #[serde(rename = "serverUrl")]
    pub server_url: String,
    #[serde(rename = "fileHash")]
    pub file_hash: String,
    #[serde(rename = "fileSize")]
    pub file_size: u64,
    #[serde(rename = "fileName")]
    pub file_name: Option<String>,
    pub sources: Option<Vec<String>>,
    pub timeout: Option<u64>,
    #[serde(rename = "chunkHashes")]
    pub chunk_hashes: Option<Vec<String>>,
}

/// Complete metadata combining GossipSub data from a single seeder
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeederCompleteMetadata {
    pub general: SeederGeneralInfo,
    pub file_specific: SeederFileInfo,
}

/// GossipSub subscription and cache manager
pub struct GossipSubManager {
    general_info_cache: Arc<RwLock<HashMap<String, SeederGeneralInfo>>>,
    file_info_cache: Arc<RwLock<HashMap<String, HashMap<String, SeederFileInfo>>>>, // file_hash -> peer_id -> SeederFileInfo
}

impl GossipSubManager {
    pub fn new() -> Self {
        Self {
            general_info_cache: Arc::new(RwLock::new(HashMap::new())),
            file_info_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Cache general seeder info
    pub async fn cache_general_info(&self, info: SeederGeneralInfo) {
        let peer_id = info.peer_id.clone();
        let mut cache = self.general_info_cache.write().await;
        debug!("Caching general info for peer {}", peer_id);
        cache.insert(peer_id, info);
    }

    /// Cache file-specific info
    pub async fn cache_file_info(&self, info: SeederFileInfo) {
        let peer_id = info.peer_id.clone();
        let file_hash = info.file_hash.clone();
        let mut cache = self.file_info_cache.write().await;

        debug!(
            "Caching file info for peer {} and file {}",
            peer_id, file_hash
        );

        cache
            .entry(file_hash)
            .or_insert_with(HashMap::new)
            .insert(peer_id, info);
    }

    /// Collect metadata for a specific file from all providers with timeout
    pub async fn collect_metadata_with_timeout(
        &self,
        providers: &[PeerId],
        file_hash: &str,
        timeout_secs: u64,
    ) -> HashMap<String, SeederCompleteMetadata> {
        let timeout_duration = Duration::from_secs(timeout_secs);

        match timeout(
            timeout_duration,
            self.collect_metadata(providers, file_hash),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => {
                warn!(
                    "Timeout after {}s collecting metadata for file {}",
                    timeout_secs, file_hash
                );
                HashMap::new()
            }
        }
    }

    /// Collect metadata for a specific file from all providers
    async fn collect_metadata(
        &self,
        providers: &[PeerId],
        file_hash: &str,
    ) -> HashMap<String, SeederCompleteMetadata> {
        let mut result = HashMap::new();

        let general_cache = self.general_info_cache.read().await;
        let file_cache = self.file_info_cache.read().await;

        for provider in providers {
            let peer_id_str = provider.to_string();

            // Get general info
            let general_info = match general_cache.get(&peer_id_str) {
                Some(info) => info.clone(),
                None => {
                    debug!("No general info cached for peer {}", peer_id_str);
                    continue;
                }
            };

            // Get file-specific info
            let file_info = match file_cache.get(file_hash).and_then(|m| m.get(&peer_id_str)) {
                Some(info) => info.clone(),
                None => {
                    debug!(
                        "No file info cached for peer {} and file {}",
                        peer_id_str, file_hash
                    );
                    continue;
                }
            };

            result.insert(
                peer_id_str.clone(),
                SeederCompleteMetadata {
                    general: general_info,
                    file_specific: file_info,
                },
            );
        }

        info!(
            "Collected metadata from {} providers for file {}",
            result.len(),
            file_hash
        );

        info!("{:?}", result);

        result
    }

    /// Get general info for a single seeder (non-blocking)
    pub async fn get_general_info(&self, peer_id: &str) -> Option<SeederGeneralInfo> {
        let cache = self.general_info_cache.read().await;
        cache.get(peer_id).cloned()
    }

    /// Get file info for a single seeder (non-blocking)
    pub async fn get_file_info(&self, file_hash: &str, peer_id: &str) -> Option<SeederFileInfo> {
        let cache = self.file_info_cache.read().await;
        cache.get(file_hash).and_then(|m| m.get(peer_id).cloned())
    }

    /// Check if we have complete metadata for a seeder
    pub async fn has_complete_metadata(&self, file_hash: &str, peer_id: &str) -> bool {
        let has_general = self.get_general_info(peer_id).await.is_some();
        let has_file = self.get_file_info(file_hash, peer_id).await.is_some();
        has_general && has_file
    }

    /// Clear old entries from cache (call periodically to prevent memory bloat)
    pub async fn cleanup_old_entries(&self, max_age_secs: u64) {
        let now = unix_timestamp();
        let cutoff = now.saturating_sub(max_age_secs);

        // Clean general info cache
        let mut general_cache = self.general_info_cache.write().await;
        general_cache.retain(|peer_id, info| {
            let keep = info.timestamp >= cutoff;
            if !keep {
                debug!("Removing stale general info for peer {}", peer_id);
            }
            keep
        });

        // Clean file info cache
        let mut file_cache = self.file_info_cache.write().await;
        file_cache.retain(|file_hash, peer_map| {
            peer_map.retain(|peer_id, info| {
                let keep = info.timestamp >= cutoff;
                if !keep {
                    debug!(
                        "Removing stale file info for peer {} and file {}",
                        peer_id, file_hash
                    );
                }
                keep
            });
            !peer_map.is_empty()
        });

        info!(
            "Cleanup complete: {} general entries, {} file entries",
            general_cache.len(),
            file_cache.len()
        );
    }
}

/// Topic naming functions

pub fn general_seeder_topic(peer_id: &PeerId) -> IdentTopic {
    IdentTopic::new(format!("seeder/{}", peer_id))
}

pub fn file_seeder_topic(peer_id: &PeerId, file_hash: &str) -> IdentTopic {
    IdentTopic::new(format!("seeder/{}/file/{}", peer_id, file_hash))
}

/// Helper functions

pub fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System time before Unix epoch")
        .as_secs()
}

/// Derive supported protocols from FileMetadata
pub fn derive_protocols(metadata: &FileMetadata) -> Vec<String> {
    let mut protocols = Vec::new();

    if metadata.cids.is_some() && !metadata.cids.as_ref().unwrap().is_empty() {
        protocols.push("bitswap".to_string());
    }

    if metadata.http_sources.is_some() && !metadata.http_sources.as_ref().unwrap().is_empty() {
        protocols.push("http".to_string());
    }

    if metadata.ftp_sources.is_some() && !metadata.ftp_sources.as_ref().unwrap().is_empty() {
        protocols.push("ftp".to_string());
    }

    if metadata.ed2k_sources.is_some() && !metadata.ed2k_sources.as_ref().unwrap().is_empty() {
        protocols.push("ed2k".to_string());
    }

    if metadata.info_hash.is_some() {
        protocols.push("bittorrent".to_string());
    }

    protocols
}

impl From<FileMetadata> for ProtocolDetails {
    fn from(metadata: FileMetadata) -> Self {
        // Build HTTP protocol details if HTTP sources exist
        let http = metadata.http_sources.and_then(|sources| {
            if sources.is_empty() {
                None
            } else {
                Some(HttpProtocolDetails {
                    sources: sources
                        .into_iter()
                        .map(|s| HttpSourceInfo {
                            url: s.url,
                            auth_header: s.auth_header,
                            verify_ssl: s.verify_ssl,
                            headers: s.headers,
                            timeout_secs: s.timeout_secs,
                        })
                        .collect(),
                })
            }
        });

        // Build FTP protocol details if FTP sources exist
        let ftp = metadata.ftp_sources.and_then(|sources| {
            if sources.is_empty() {
                None
            } else {
                Some(FtpProtocolDetails {
                    sources: sources
                        .into_iter()
                        .map(|s| FtpSourceInfo {
                            url: s.url,
                            username: s.username,
                            encrypted_password: None, // Not set when converting from FileMetadata
                            passive_mode: true,       // Default to passive mode
                            use_ftps: false,          // Default to regular FTP
                            timeout_secs: Some(30),   // Default timeout
                            supports_resume: s.supports_resume,
                            file_size: s.file_size,
                            last_checked: s.last_checked,
                            is_available: s.is_available,
                        })
                        .collect(),
                })
            }
        });

        // Build ED2K protocol details if ED2K sources exist
        let ed2k = metadata.ed2k_sources.and_then(|sources| {
            if sources.is_empty() {
                None
            } else {
                Some(Ed2kProtocolDetails {
                    sources: sources
                        .into_iter()
                        .map(|s| Ed2kSourceInfo {
                            server_url: s.server_url,
                            file_hash: s.file_hash,
                            file_size: s.file_size,
                            file_name: s.file_name,
                            sources: s.sources,
                            timeout: s.timeout,
                            chunk_hashes: s.chunk_hashes,
                        })
                        .collect(),
                })
            }
        });

        // Build BitTorrent protocol details if info_hash exists
        let bittorrent = metadata.info_hash.map(|info_hash| BitTorrentProtocolDetails {
            info_hash,
            trackers: metadata.trackers.unwrap_or_default(),
        });

        // Build BitSwap protocol details if CIDs exist
        let bitswap = metadata.cids.and_then(|cids| {
            if cids.is_empty() {
                None
            } else {
                Some(BitswapProtocolDetails {
                    cids: cids.into_iter().map(|cid| cid.to_string()).collect(),
                    is_root: metadata.is_root,
                })
            }
        });

        // Build WebRTC protocol details (enabled by default for peer-to-peer transfers)
        let webrtc = Some(WebRtcProtocolDetails { enabled: true });

        // Build encryption details if file is encrypted
        let encryption = if metadata.is_encrypted {
            match (
                metadata.encryption_method,
                metadata.key_fingerprint,
                metadata.encrypted_key_bundle,
            ) {
                (Some(method), Some(key_fingerprint), Some(encrypted_key_bundle)) => {
                    Some(EncryptionDetails {
                        method,
                        key_fingerprint,
                        encrypted_key_bundle,
                    })
                }
                _ => None,
            }
        } else {
            None
        };

        Self {
            http,
            ftp,
            ed2k,
            bittorrent,
            bitswap,
            webrtc,
            encryption,
        }
    }
}
