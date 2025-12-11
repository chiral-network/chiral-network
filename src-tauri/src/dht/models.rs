pub use cid::Cid;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::SystemTime;

// internal crate imports - assumed to exist based on original file
use crate::download_source::HttpSourceInfo;
use crate::encryption::EncryptedAesKeyBundle;

// Custom serializer for CIDs to convert them to strings for frontend compatibility
fn serialize_cids_as_strings<S>(
    cids: &Option<Vec<Cid>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match cids {
        Some(cid_vec) => {
            let strings: Vec<String> = cid_vec.iter().map(|cid| cid.to_string()).collect();
            serializer.serialize_some(&strings)
        }
        None => serializer.serialize_none(),
    }
}

// Custom deserializer for CIDs to convert from strings back to CID objects
fn deserialize_cids_from_strings<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<Cid>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt_strings: Option<Vec<String>> = Option::deserialize(deserializer)?;
    match opt_strings {
        Some(strings) => {
            let mut cids = Vec::new();
            for cid_str in strings {
                match cid_str.parse::<Cid>() {
                    Ok(cid) => cids.push(cid),
                    Err(e) => {
                        return Err(serde::de::Error::custom(format!(
                            "Failed to parse CID from string '{}': {}",
                            cid_str, e
                        )));
                    }
                }
            }
            Ok(Some(cids))
        }
        None => Ok(None),
    }
}

// =========================================================================
// Error Types
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Ed2kError {
    InvalidLink(String),
    MissingPart(&'static str),
    InvalidFileSize(String),
}

impl std::fmt::Display for Ed2kError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ed2kError::InvalidLink(s) => write!(f, "Invalid ed2k link format: {}", s),
            Ed2kError::MissingPart(s) => write!(f, "Missing required part in link: {}", s),
            Ed2kError::InvalidFileSize(s) => write!(f, "Invalid file size: {}", s),
        }
    }
}

impl std::error::Error for Ed2kError {}

// =========================================================================
// File Metadata & Sources
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileMetadata {
    /// The Merkle root of the original file chunks, used as the primary identifier for integrity.
    #[serde(rename = "merkleRoot")]
    pub merkle_root: String,

    #[serde(rename = "fileName")]
    pub file_name: String,

    #[serde(rename = "fileSize")]
    pub file_size: u64,

    #[serde(skip)]
    pub file_data: Vec<u8>, // holds the actual file data

    #[serde(default)]
    pub seeders: Vec<String>,

    #[serde(default, rename = "createdAt")]
    pub created_at: u64,

    #[serde(default, skip_serializing_if = "Option::is_none", rename = "mimeType")]
    pub mime_type: Option<String>,

    /// Whether the file is encrypted
    #[serde(default, rename = "isEncrypted")]
    pub is_encrypted: bool,

    /// The encryption method used (e.g., "AES-256-GCM")
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "encryptionMethod")]
    pub encryption_method: Option<String>,

    /// Fingerprint of the encryption key for identification.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "keyFingerprint")]
    pub key_fingerprint: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none", rename = "parentHash")]
    pub parent_hash: Option<String>,

    /// The root CID(s) for retrieving the file from Bitswap. Usually one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(serialize_with = "serialize_cids_as_strings")]
    #[serde(deserialize_with = "deserialize_cids_from_strings")]
    pub cids: Option<Vec<Cid>>,

    /// For encrypted files, this contains the encrypted AES key and other info.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "encryptedKeyBundle")]
    pub encrypted_key_bundle: Option<EncryptedAesKeyBundle>,

    #[serde(default, skip_serializing_if = "Option::is_none", rename = "ftpSources")]
    pub ftp_sources: Option<Vec<FtpSourceInfo>>,

    // ed2k sources for downloading the file
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "ed2kSources")]
    pub ed2k_sources: Option<Vec<Ed2kSourceInfo>>,

    /// HTTP sources for downloading the file (HTTP Range request endpoints)
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "httpSources")]
    pub http_sources: Option<Vec<HttpSourceInfo>>,

    #[serde(default)]
    pub is_root: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_path: Option<String>,

    /// Price in Chiral tokens set by the uploader
    #[serde(default)]
    pub price: f64,

    /// Ethereum address of the uploader (for payment)
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "uploaderAddress")]
    pub uploader_address: Option<String>,

    /// The SHA-1 info hash for BitTorrent compatibility.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "infoHash")]
    pub info_hash: Option<String>,

    /// A list of BitTorrent tracker URLs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trackers: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FtpSourceInfo {
    pub url: String,
    pub username: Option<String>,
    /// Optional password (stored temporarily, not persisted to DHT for security)
    #[serde(skip_serializing, skip_deserializing)]
    pub password: Option<String>,
    #[serde(rename = "supportsResume")]
    pub supports_resume: bool,
    #[serde(rename = "fileSize")]
    pub file_size: u64,
    #[serde(rename = "lastChecked")]
    pub last_checked: Option<u64>, // Unix timestamp
    #[serde(rename = "isAvailable")]
    pub is_available: bool,
}

impl FtpSourceInfo {
    /// Creates a copy of the struct suitable for DHT storage, stripping the password.
    pub fn for_dht_storage(&self) -> Self {
        Self {
            url: self.url.clone(),
            username: self.username.clone(),
            password: None, // Always None for DHT storage
            supports_resume: self.supports_resume,
            file_size: self.file_size,
            last_checked: self.last_checked,
            is_available: self.is_available,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ed2kSourceInfo {
    /// ed2k server URL (e.g., "ed2k://|server|1.2.3.4|4661|/")
    pub server_url: String,
    /// ed2k file hash (MD4 hash in hex)
    pub file_hash: String,
    pub file_size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
    /// ED2K chunk hashes (MD4 hashes for each 9.28MB chunk)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk_hashes: Option<Vec<String>>,
}
#[derive(Debug, Clone, serde::Serialize)]
pub struct Ed2kDownloadStatus {
    pub progress: f32,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub state: String,
}
impl Ed2kSourceInfo {
    pub fn from_ed2k_link(link: &str) -> Result<Self, Ed2kError> {
        let Some(parts_str) = link.strip_prefix("ed2k://|") else {
            return Err(Ed2kError::InvalidLink(link.to_string()));
        };

        let clean_parts_str = parts_str.trim_end_matches(&['/', '|']);
        let parts: Vec<&str> = clean_parts_str.split('|').collect();

        if parts.is_empty() {
            return Err(Ed2kError::InvalidLink(link.to_string()));
        }

        match parts[0] {
            "file" => {
                if parts.len() < 4 {
                    return Err(Ed2kError::MissingPart(
                        "File link requires name, size, and hash",
                    ));
                }
                let file_name = parts[1].to_string();
                let file_size_str = parts[2];
                let file_hash = parts[3].to_string();
                let file_size = file_size_str
                    .parse::<u64>()
                    .map_err(|_| Ed2kError::InvalidFileSize(file_size_str.to_string()))?;

                Ok(Self {
                    server_url: String::new(),
                    file_hash,
                    file_size,
                    file_name: Some(file_name),
                    sources: None,
                    timeout: None,
                    chunk_hashes: None,
                })
            }
            "server" => {
                if parts.len() < 3 {
                    return Err(Ed2kError::MissingPart("Server link requires ip and port"));
                }
                let ip = parts[1];
                let port = parts[2];
                let server_url = format!("ed2k://|server|{}|{}|/", ip, port);

                Ok(Self {
                    server_url,
                    file_hash: String::new(),
                    file_size: 0,
                    file_name: None,
                    sources: None,
                    timeout: None,
                    chunk_hashes: None,
                })
            }
            _ => Err(Ed2kError::InvalidLink(format!(
                "Unknown link type: {}",
                parts[0]
            ))),
        }
    }
}

// =========================================================================
// Heartbeat & Seeding
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeederHeartbeat {
    pub peer_id: String,
    pub expires_at: u64,
    pub last_heartbeat: u64,
}

#[derive(Debug, Clone)]
pub struct FileHeartbeatCacheEntry {
    pub heartbeats: Vec<SeederHeartbeat>,
    pub metadata: serde_json::Value,
}

// =========================================================================
// Magnet URI
// =========================================================================

#[derive(Debug, PartialEq, Eq)]
pub struct MagnetData {
    pub info_hash: String,
    pub display_name: Option<String>,
    pub trackers: Vec<String>,
}

// =========================================================================
// NAT & Network Metrics
// =========================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NatReachabilityState {
    Unknown,
    Public,
    Private,
}

impl Default for NatReachabilityState {
    fn default() -> Self {
        NatReachabilityState::Unknown
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NatConfidence {
    Low,
    Medium,
    High,
}

impl Default for NatConfidence {
    fn default() -> Self {
        NatConfidence::Low
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NatHistoryItem {
    pub state: NatReachabilityState,
    pub confidence: NatConfidence,
    pub timestamp: u64,
    pub summary: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ReachabilityRecord {
    pub state: NatReachabilityState,
    pub confidence: NatConfidence,
    pub timestamp: SystemTime,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct DhtMetrics {
    pub last_bootstrap: Option<SystemTime>,
    pub last_success: Option<SystemTime>,
    pub last_error_at: Option<SystemTime>,
    pub last_error: Option<String>,
    pub bootstrap_failures: u64,
    pub listen_addrs: Vec<String>,
    pub reachability_state: NatReachabilityState,
    pub reachability_confidence: NatConfidence,
    pub last_reachability_change: Option<SystemTime>,
    pub last_probe_at: Option<SystemTime>,
    pub last_reachability_error: Option<String>,
    pub observed_addrs: Vec<String>,
    pub reachability_history: VecDeque<ReachabilityRecord>,
    pub success_streak: u32,
    pub failure_streak: u32,
    pub autonat_enabled: bool,
    // AutoRelay metrics
    pub autorelay_enabled: bool,
    pub last_autorelay_enabled_at: Option<SystemTime>,
    pub last_autorelay_disabled_at: Option<SystemTime>,
    pub active_relay_peer_id: Option<String>,
    pub relay_reservation_status: Option<String>,
    pub last_reservation_success: Option<SystemTime>,
    pub last_reservation_failure: Option<SystemTime>,
    pub reservation_renewals: u64,
    pub reservation_evictions: u64,
    // DCUtR metrics
    pub dcutr_enabled: bool,
    pub dcutr_hole_punch_attempts: u64,
    pub dcutr_hole_punch_successes: u64,
    pub dcutr_hole_punch_failures: u64,
    pub last_dcutr_success: Option<SystemTime>,
    pub last_dcutr_failure: Option<SystemTime>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DhtMetricsSnapshot {
    pub peer_count: usize,
    pub last_bootstrap: Option<u64>,
    pub last_peer_event: Option<u64>,
    pub last_error: Option<String>,
    pub last_error_at: Option<u64>,
    pub bootstrap_failures: u64,
    pub listen_addrs: Vec<String>,
    pub relay_listen_addrs: Vec<String>,
    pub reachability: NatReachabilityState,
    pub reachability_confidence: NatConfidence,
    pub last_reachability_change: Option<u64>,
    pub last_probe_at: Option<u64>,
    pub last_reachability_error: Option<String>,
    pub observed_addrs: Vec<String>,
    pub reachability_history: Vec<NatHistoryItem>,
    pub autonat_enabled: bool,
    // AutoRelay metrics
    pub autorelay_enabled: bool,
    pub last_autorelay_enabled_at: Option<u64>,
    pub last_autorelay_disabled_at: Option<u64>,
    pub active_relay_peer_id: Option<String>,
    pub relay_reservation_status: Option<String>,
    pub last_reservation_success: Option<u64>,
    pub last_reservation_failure: Option<u64>,
    pub reservation_renewals: u64,
    pub reservation_evictions: u64,
    // DCUtR metrics
    pub dcutr_enabled: bool,
    pub dcutr_hole_punch_attempts: u64,
    pub dcutr_hole_punch_successes: u64,
    pub dcutr_hole_punch_failures: u64,
    pub last_dcutr_success: Option<u64>,
    pub last_dcutr_failure: Option<u64>,
}
