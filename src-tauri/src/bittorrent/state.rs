use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Distinguishes how the torrent was identified when added.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TorrentIdentifier {
    MagnetLink(String),
    TorrentFile(PathBuf),
    InfoHash(String),
}

/// Operating mode for the torrent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TorrentMode {
    Download,
    Seed,
}

/// Persistable representation of a torrent managed by the handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistentTorrent {
    pub info_hash: String,
    pub identifier: TorrentIdentifier,
    pub magnet_link: Option<String>,
    pub torrent_path: Option<PathBuf>,
    pub download_dir: PathBuf,
    pub mode: TorrentMode,
    pub added_at: Option<DateTime<Utc>>,
}
