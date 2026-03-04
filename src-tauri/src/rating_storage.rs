use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A single rating from a downloader to a seeder.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rating {
    pub id: String,
    /// Wallet address of the seeder being rated
    pub seeder_wallet: String,
    /// Wallet address of the downloader who is rating
    pub rater_wallet: String,
    /// Hash of the file that was downloaded
    pub file_hash: String,
    /// Score from 1 to 5
    pub score: u8,
    /// Optional short comment (max 500 chars)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// Unix timestamp in seconds
    pub created_at: u64,
}

/// Persisted rating manifest.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RatingManifest {
    pub ratings: Vec<Rating>,
}

/// Shared state for the rating system.
#[derive(Clone)]
pub struct RatingState {
    pub manifest: Arc<RwLock<RatingManifest>>,
    data_dir: PathBuf,
}

impl RatingState {
    pub fn new(data_dir: PathBuf) -> Self {
        let manifest = load_manifest(&data_dir);
        Self {
            manifest: Arc::new(RwLock::new(manifest)),
            data_dir,
        }
    }

    pub async fn persist(&self) {
        let m = self.manifest.read().await;
        save_manifest(&self.data_dir, &m);
    }
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

fn ratings_dir(data_dir: &PathBuf) -> PathBuf {
    data_dir.join("chiral-ratings")
}

fn manifest_path(data_dir: &PathBuf) -> PathBuf {
    ratings_dir(data_dir).join("ratings.json")
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

fn load_manifest(data_dir: &PathBuf) -> RatingManifest {
    let path = manifest_path(data_dir);
    let Ok(data) = std::fs::read_to_string(&path) else {
        return RatingManifest::default();
    };
    serde_json::from_str(&data).unwrap_or_default()
}

fn save_manifest(data_dir: &PathBuf, manifest: &RatingManifest) {
    let path = manifest_path(data_dir);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(manifest) {
        let _ = std::fs::write(&path, json);
    }
}

/// Current Unix timestamp in seconds.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Generate a UUID v4 string.
pub fn generate_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rating_serialization() {
        let rating = Rating {
            id: "test-id".into(),
            seeder_wallet: "0xABC".into(),
            rater_wallet: "0xDEF".into(),
            file_hash: "abc123".into(),
            score: 4,
            comment: Some("Great speed!".into()),
            created_at: 1700000000,
        };
        let json = serde_json::to_string(&rating).unwrap();
        let parsed: Rating = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.score, 4);
        assert_eq!(parsed.seeder_wallet, "0xABC");
    }

    #[test]
    fn test_manifest_default() {
        let m = RatingManifest::default();
        assert!(m.ratings.is_empty());
    }
}
