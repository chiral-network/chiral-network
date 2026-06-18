use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A single item (file or folder) in the Drive.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveItem {
    pub id: String,
    pub name: String,
    pub item_type: String, // "file" or "folder"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    pub created_at: u64,
    pub modified_at: u64,
    #[serde(default)]
    pub starred: bool,
    /// Relative path within drive_files_dir (files only)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_path: Option<String>,
    /// Wallet address of the owner
    #[serde(default)]
    pub owner: String,
    /// Whether the file is publicly accessible via share links.
    /// When false, all share links for this item are blocked.
    #[serde(default = "default_true")]
    pub is_public: bool,

    // ── Seeding metadata (optional, only set for files published to DHT) ──
    /// SHA-256 Merkle root from DHT publishing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merkle_root: Option<String>,
    /// Transfer protocol: "WebRTC" or "BitTorrent".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    /// Price in CHI tokens (as string, "0" = free).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_chi: Option<String>,
    /// Wallet that receives folder-level payments. Only set for folder
    /// sale items; legacy manifests fall back to `owner`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payment_wallet: Option<String>,
    /// Whether this file should auto-seed whenever DHT is running.
    /// This is persisted user intent.
    #[serde(default)]
    pub seed_enabled: bool,
    /// Whether the file is actively being seeded on the P2P network.
    /// This is runtime state and may be false while DHT is offline.
    #[serde(default)]
    pub seeding: bool,
}

fn default_true() -> bool {
    true
}

/// A share link granting access to a DriveItem.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareLink {
    /// 16-char alphanumeric token
    pub id: String,
    /// The DriveItem being shared
    pub item_id: String,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    /// Required payment in CHI to unlock this share in browser.
    #[serde(default = "default_share_price")]
    pub price_chi: String,
    /// Recipient wallet that receives share-link payments.
    #[serde(default)]
    pub recipient_wallet: String,
    #[serde(default)]
    pub is_public: bool,
    #[serde(default)]
    pub download_count: u64,
}

fn default_share_price() -> String {
    "0".to_string()
}

/// Persisted drive manifest.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DriveManifest {
    pub items: Vec<DriveItem>,
    pub shares: Vec<ShareLink>,
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// Base directory for drive data.
pub fn drive_base_dir() -> Option<PathBuf> {
    Some(crate::network::data_dir().join("chiral-drive"))
}

/// Directory where uploaded files are stored.
pub fn drive_files_dir() -> Option<PathBuf> {
    drive_base_dir().map(|d| d.join("files"))
}

/// Path to the drive manifest JSON.
fn manifest_path() -> Option<PathBuf> {
    drive_base_dir().map(|d| d.join("manifest.json"))
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

pub fn load_manifest() -> DriveManifest {
    let Some(path) = manifest_path() else {
        return DriveManifest::default();
    };
    load_manifest_from_path(&path)
}

fn load_manifest_from_path(path: &Path) -> DriveManifest {
    let data = match std::fs::read_to_string(path) {
        Ok(data) => data,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return DriveManifest::default(),
        Err(e) => {
            eprintln!(
                "[Drive] Failed to read Drive manifest {}: {}; starting with an empty manifest",
                path.display(),
                e
            );
            return DriveManifest::default();
        }
    };

    match serde_json::from_str(&data) {
        Ok(manifest) => manifest,
        Err(e) => {
            match quarantine_malformed_manifest(path) {
                Ok(quarantine) => eprintln!(
                    "[Drive] Malformed Drive manifest {} quarantined at {}: {}",
                    path.display(),
                    quarantine.display(),
                    e
                ),
                Err(quarantine_err) => eprintln!(
                    "[Drive] Malformed Drive manifest {} could not be quarantined: {}; starting with an empty manifest",
                    path.display(),
                    quarantine_err
                ),
            }
            DriveManifest::default()
        }
    }
}

fn quarantine_malformed_manifest(path: &Path) -> Result<PathBuf, String> {
    let quarantine = malformed_manifest_quarantine_path(path)?;
    std::fs::rename(path, &quarantine).map_err(|e| {
        format!(
            "rename {} to {}: {}",
            path.display(),
            quarantine.display(),
            e
        )
    })?;
    Ok(quarantine)
}

fn malformed_manifest_quarantine_path(path: &Path) -> Result<PathBuf, String> {
    malformed_manifest_quarantine_path_at(path, std::time::SystemTime::now())
}

fn malformed_manifest_quarantine_path_at(
    path: &Path,
    now: std::time::SystemTime,
) -> Result<PathBuf, String> {
    let timestamp = now_secs_at(now)?;
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("manifest.json");
    for attempt in 0..1000 {
        let suffix = if attempt == 0 {
            format!("malformed-{timestamp}")
        } else {
            format!("malformed-{timestamp}-{attempt}")
        };
        let candidate = path.with_file_name(format!("{file_name}.{suffix}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Ok(path.with_file_name(format!("{file_name}.malformed-{timestamp}-overflow")))
}

pub fn save_manifest(manifest: &DriveManifest) {
    let Some(path) = manifest_path() else { return };
    if let Err(e) = save_manifest_to_path(manifest, &path) {
        eprintln!(
            "[Drive] Failed to save Drive manifest {}: {}",
            path.display(),
            e
        );
    }
}

fn save_manifest_to_path(manifest: &DriveManifest, path: &Path) -> Result<(), String> {
    match std::fs::read_to_string(path) {
        Ok(data) => {
            if serde_json::from_str::<DriveManifest>(&data).is_err() {
                return Err(format!(
                    "refusing to overwrite malformed Drive manifest at {}; fix or remove it manually",
                    path.display()
                ));
            }
        }
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) => {}
        Err(_) if path.is_dir() => {}
        Err(e) => {
            return Err(format!(
                "failed to inspect existing Drive manifest {}: {}",
                path.display(),
                e
            ));
        }
    }
    let json = serde_json::to_string_pretty(manifest)
        .map_err(|e| format!("serialize Drive manifest: {}", e))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            format!(
                "create Drive manifest directory {}: {}",
                parent.display(),
                e
            )
        })?;
    }
    std::fs::write(path, json)
        .map_err(|e| format!("write Drive manifest {}: {}", path.display(), e))
}

// ---------------------------------------------------------------------------
// ID / token generation
// ---------------------------------------------------------------------------

pub fn generate_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Generate a 16-char alphanumeric share token.
pub fn generate_share_token() -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    (0..16)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

/// Current Unix timestamp in seconds.
pub fn now_secs() -> Result<u64, String> {
    now_secs_at(std::time::SystemTime::now())
}

pub fn now_secs_at(now: std::time::SystemTime) -> Result<u64, String> {
    now.duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|err| {
            format!(
                "Cannot generate Drive timestamp because the system clock is before UNIX_EPOCH: {}",
                err
            )
        })
}

/// Guess MIME type from file extension.
pub fn mime_from_name(name: &str) -> String {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "application/javascript",
        "json" => "application/json",
        "txt" => "text/plain",
        "md" => "text/markdown",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// Collect all descendant item IDs (recursive) for a given parent.
pub fn collect_descendants(parent_id: &str, items: &[DriveItem]) -> Vec<String> {
    let mut result = vec![parent_id.to_string()];
    let mut queue = vec![parent_id.to_string()];
    while let Some(pid) = queue.pop() {
        for item in items {
            if item.parent_id.as_deref() == Some(&pid) {
                result.push(item.id.clone());
                if item.item_type == "folder" {
                    queue.push(item.id.clone());
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_drive_item(id: &str) -> DriveItem {
        DriveItem {
            id: id.into(),
            name: "Test file".into(),
            item_type: "file".into(),
            parent_id: None,
            size: Some(100),
            mime_type: None,
            created_at: 0,
            modified_at: 0,
            starred: false,
            storage_path: Some("files/test".into()),
            owner: "test-owner".into(),
            is_public: true,
            merkle_root: None,
            protocol: None,
            price_chi: None,
            payment_wallet: None,
            seed_enabled: true,
            seeding: true,
        }
    }

    #[test]
    fn load_manifest_missing_file_starts_empty() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("manifest.json");

        let manifest = load_manifest_from_path(&path);

        assert!(manifest.items.is_empty());
        assert!(manifest.shares.is_empty());
        assert!(!path.exists());
    }

    #[test]
    fn load_manifest_reads_valid_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("manifest.json");
        let manifest = DriveManifest {
            items: vec![test_drive_item("item-1")],
            shares: Vec::new(),
        };
        save_manifest_to_path(&manifest, &path).expect("valid manifest should save");

        let loaded = load_manifest_from_path(&path);

        assert_eq!(loaded.items.len(), 1);
        assert_eq!(loaded.items[0].id, "item-1");
        assert!(path.exists());
    }

    #[test]
    fn load_manifest_quarantines_malformed_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("manifest.json");
        let malformed = "{not valid json";
        std::fs::write(&path, malformed).unwrap();

        let loaded = load_manifest_from_path(&path);

        assert!(loaded.items.is_empty());
        assert!(loaded.shares.is_empty());
        assert!(!path.exists());

        let quarantines: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("manifest.json.malformed-")
            })
            .collect();
        assert_eq!(quarantines.len(), 1);
        assert_eq!(
            std::fs::read_to_string(quarantines[0].path()).unwrap(),
            malformed
        );
    }

    #[test]
    fn now_secs_at_preserves_seconds() {
        let ts = now_secs_at(std::time::UNIX_EPOCH + std::time::Duration::from_secs(42))
            .expect("post-epoch Drive timestamp should be valid");

        assert_eq!(ts, 42);
    }

    #[test]
    fn now_secs_at_rejects_pre_epoch_clock() {
        let err = now_secs_at(std::time::UNIX_EPOCH - std::time::Duration::from_secs(1))
            .expect_err("pre-epoch Drive timestamp should be rejected");

        assert!(err.contains("Drive timestamp"));
        assert!(err.contains("system clock is before UNIX_EPOCH"));
    }

    #[test]
    fn malformed_manifest_quarantine_path_at_uses_timestamp_suffix() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("manifest.json");

        let quarantine = malformed_manifest_quarantine_path_at(
            &path,
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(42),
        )
        .expect("post-epoch quarantine timestamp should be valid");

        assert_eq!(
            quarantine.file_name().and_then(|s| s.to_str()),
            Some("manifest.json.malformed-42")
        );
    }

    #[test]
    fn malformed_manifest_quarantine_path_at_rejects_pre_epoch_clock() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("manifest.json");
        let err = malformed_manifest_quarantine_path_at(
            &path,
            std::time::UNIX_EPOCH - std::time::Duration::from_secs(1),
        )
        .expect_err("pre-epoch quarantine timestamp should be rejected");

        assert!(err.contains("system clock is before UNIX_EPOCH"));
    }

    #[test]
    fn save_manifest_refuses_to_overwrite_malformed_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("manifest.json");
        let malformed = "{still not valid json";
        std::fs::write(&path, malformed).unwrap();
        let manifest = DriveManifest {
            items: vec![test_drive_item("replacement")],
            shares: Vec::new(),
        };

        let err = save_manifest_to_path(&manifest, &path)
            .expect_err("malformed manifest should not be overwritten");

        assert!(err.contains("refusing to overwrite malformed Drive manifest"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), malformed);
    }

    #[test]
    fn save_manifest_to_path_persists_valid_manifest() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested").join("manifest.json");
        let manifest = DriveManifest {
            items: vec![test_drive_item("saved")],
            shares: Vec::new(),
        };

        save_manifest_to_path(&manifest, &path).expect("valid manifest should save");

        let loaded = load_manifest_from_path(&path);
        assert_eq!(loaded.items.len(), 1);
        assert_eq!(loaded.items[0].id, "saved");
    }

    #[test]
    fn save_manifest_to_path_surfaces_directory_creation_failure() {
        let dir = tempdir().unwrap();
        let parent = dir.path().join("not-a-directory");
        std::fs::write(&parent, "blocking file").unwrap();
        let path = parent.join("manifest.json");
        let manifest = DriveManifest {
            items: vec![test_drive_item("item-1")],
            shares: Vec::new(),
        };

        let err = save_manifest_to_path(&manifest, &path)
            .expect_err("directory creation failure should surface");

        assert!(err.contains("create Drive manifest directory"));
    }

    #[test]
    fn save_manifest_to_path_surfaces_write_failure() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("manifest.json");
        std::fs::create_dir(&path).unwrap();
        let manifest = DriveManifest {
            items: vec![test_drive_item("item-1")],
            shares: Vec::new(),
        };

        let err =
            save_manifest_to_path(&manifest, &path).expect_err("write failure should surface");

        assert!(err.contains("write Drive manifest"));
    }

    #[test]
    fn test_generate_share_token() {
        let token = generate_share_token();
        assert_eq!(token.len(), 16);
        assert!(token.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn test_collect_descendants() {
        let items = vec![
            DriveItem {
                id: "root".into(),
                name: "Root".into(),
                item_type: "folder".into(),
                parent_id: None,
                size: None,
                mime_type: None,
                created_at: 0,
                modified_at: 0,
                starred: false,
                storage_path: None,
                owner: "test-owner".into(),
                is_public: true,
                merkle_root: None,
                protocol: None,
                price_chi: None,
                payment_wallet: None,
                seed_enabled: false,
                seeding: false,
            },
            DriveItem {
                id: "child1".into(),
                name: "Child".into(),
                item_type: "file".into(),
                parent_id: Some("root".into()),
                size: Some(100),
                mime_type: None,
                created_at: 0,
                modified_at: 0,
                starred: false,
                storage_path: None,
                owner: "test-owner".into(),
                is_public: true,
                merkle_root: None,
                protocol: None,
                price_chi: None,
                payment_wallet: None,
                seed_enabled: false,
                seeding: false,
            },
            DriveItem {
                id: "subfolder".into(),
                name: "Sub".into(),
                item_type: "folder".into(),
                parent_id: Some("root".into()),
                size: None,
                mime_type: None,
                created_at: 0,
                modified_at: 0,
                starred: false,
                storage_path: None,
                owner: "test-owner".into(),
                is_public: true,
                merkle_root: None,
                protocol: None,
                price_chi: None,
                payment_wallet: None,
                seed_enabled: false,
                seeding: false,
            },
            DriveItem {
                id: "grandchild".into(),
                name: "Grand".into(),
                item_type: "file".into(),
                parent_id: Some("subfolder".into()),
                size: Some(50),
                mime_type: None,
                created_at: 0,
                modified_at: 0,
                starred: false,
                storage_path: None,
                owner: "test-owner".into(),
                is_public: true,
                merkle_root: None,
                protocol: None,
                price_chi: None,
                payment_wallet: None,
                seed_enabled: false,
                seeding: false,
            },
        ];
        let desc = collect_descendants("root", &items);
        assert_eq!(desc.len(), 4);
    }

    #[test]
    fn test_mime_from_name() {
        assert_eq!(mime_from_name("photo.png"), "image/png");
        assert_eq!(mime_from_name("doc.pdf"), "application/pdf");
        assert_eq!(mime_from_name("unknown.xyz"), "application/octet-stream");
    }
}
