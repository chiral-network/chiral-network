use crate::bittorrent::state::PersistentTorrent;
use serde_json::Error as SerdeError;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Error types for torrent state persistence.
#[derive(Debug, Error)]
pub enum TorrentStateError {
    #[error("I/O error while accessing torrent_state.json: {0}")]
    Io(#[from] io::Error),
    #[error("Failed to parse torrent_state.json: {0}")]
    Parse(#[from] SerdeError),
}

/// Manages loading and saving the persistent torrent list.
#[derive(Debug, Clone)]
pub struct TorrentStateManager {
    state_path: PathBuf,
}

impl TorrentStateManager {
    /// Create a new state manager pointing at the given file path.
    pub fn new<P: Into<PathBuf>>(state_path: P) -> Self {
        Self {
            state_path: state_path.into(),
        }
    }

    /// Returns the path where torrent state is stored.
    pub fn state_path(&self) -> &Path {
        &self.state_path
    }

    /// Load persisted torrents from disk. Missing or empty files return an empty list.
    pub fn load_state(&self) -> Result<Vec<PersistentTorrent>, TorrentStateError> {
        if !self.state_path.exists() {
            return Ok(Vec::new());
        }

        let metadata = fs::metadata(&self.state_path)?;
        if metadata.len() == 0 {
            return Ok(Vec::new());
        }

        let file = File::open(&self.state_path)?;
        let reader = BufReader::new(file);
        let torrents = serde_json::from_reader(reader)?;
        Ok(torrents)
    }

    /// Persist the provided torrent list to disk, atomically replacing any existing file.
    pub fn save_state(&self, torrents: &[PersistentTorrent]) -> Result<(), TorrentStateError> {
        if let Some(parent) = self
            .state_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }

        // Write to a temp file first to avoid partial writes.
        let tmp_path = self
            .state_path
            .with_extension("json.tmp");
        let file = File::create(&tmp_path)?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer_pretty(&mut writer, torrents)?;
        writer.flush()?;
        writer.get_ref().sync_all()?;

        fs::rename(&tmp_path, &self.state_path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bittorrent::state::{TorrentIdentifier, TorrentMode};
    use chrono::Utc;
    use tempfile::tempdir;

    #[test]
    fn load_missing_file_returns_empty_list() {
        let dir = tempdir().unwrap();
        let manager = TorrentStateManager::new(dir.path().join("torrent_state.json"));

        let torrents = manager.load_state().unwrap();
        assert!(torrents.is_empty());
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempdir().unwrap();
        let state_path = dir.path().join("torrent_state.json");
        let manager = TorrentStateManager::new(&state_path);

        let torrents = vec![PersistentTorrent {
            info_hash: "abc123".into(),
            identifier: TorrentIdentifier::MagnetLink("magnet:?xt=urn:btih:abc123".into()),
            magnet_link: Some("magnet:?xt=urn:btih:abc123".into()),
            torrent_path: None,
            download_dir: dir.path().join("downloads"),
            mode: TorrentMode::Download,
            added_at: Some(Utc::now()),
        }];

        manager.save_state(&torrents).unwrap();

        let loaded = manager.load_state().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].info_hash, "abc123");
        assert_eq!(loaded[0].magnet_link.as_deref(), torrents[0].magnet_link.as_deref());
        assert!(matches!(loaded[0].mode, TorrentMode::Download));
    }
}
