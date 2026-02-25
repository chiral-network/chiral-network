use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

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
    /// SHA-256 hex of the password, if password-protected
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_hash: Option<String>,
    #[serde(default)]
    pub is_public: bool,
    #[serde(default)]
    pub download_count: u64,
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
    dirs::data_dir().map(|d| d.join("chiral-network").join("chiral-drive"))
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
    let Ok(data) = std::fs::read_to_string(&path) else {
        return DriveManifest::default();
    };
    serde_json::from_str(&data).unwrap_or_default()
}

pub fn save_manifest(manifest: &DriveManifest) {
    let Some(path) = manifest_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(manifest) {
        let _ = std::fs::write(&path, json);
    }
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

/// Hash a password with SHA-256 and return hex string.
pub fn hash_password(password: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(password.as_bytes());
    hex::encode(hasher.finalize())
}

/// Current Unix timestamp in seconds.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
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

    #[test]
    fn test_generate_share_token() {
        let token = generate_share_token();
        assert_eq!(token.len(), 16);
        assert!(token.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn test_hash_password() {
        let h = hash_password("test123");
        assert_eq!(h.len(), 64); // SHA-256 hex
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
