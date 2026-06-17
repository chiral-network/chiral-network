use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A single file within a hosted site.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteFile {
    /// Relative path within the site (e.g. "index.html", "css/style.css")
    pub path: String,
    /// File size in bytes
    pub size: u64,
}

/// A hosted static website.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostedSite {
    /// Short random alphanumeric ID (8 chars)
    pub id: String,
    /// User-given name
    pub name: String,
    /// Absolute path to the site directory on disk
    pub directory: String,
    /// Unix timestamp (seconds)
    pub created_at: u64,
    /// Files in the site
    pub files: Vec<SiteFile>,
    /// URL on a relay gateway, if published (e.g. "http://130.245.173.73:8080/sites/a1b2c3d4/")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_url: Option<String>,
    /// URL on a CDN server, if uploaded for always-on hosting
    /// (e.g. "http://130.245.173.73:9420/cdn/sites/a1b2c3d4/"). Unlike
    /// the relay URL, this stays reachable when the local client is offline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cdn_url: Option<String>,
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

/// Directory where all hosted sites are stored.
pub fn sites_base_dir() -> Option<PathBuf> {
    Some(crate::network::data_dir().join("sites"))
}

/// Path to the metadata JSON file.
fn metadata_path() -> Option<PathBuf> {
    Some(crate::network::data_dir().join("hosted_sites.json"))
}

/// Load all hosted sites from disk.
pub fn load_sites() -> Vec<HostedSite> {
    let Some(path) = metadata_path() else {
        return Vec::new();
    };
    load_sites_from_path(&path)
}

fn load_sites_from_path(path: &Path) -> Vec<HostedSite> {
    let data = match std::fs::read(path) {
        Ok(data) => data,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            eprintln!(
                "[Hosting] Failed to read hosted-site metadata {}: {}; starting with no hosted sites",
                path.display(),
                e
            );
            return Vec::new();
        }
    };

    match serde_json::from_slice(&data) {
        Ok(sites) => sites,
        Err(e) => {
            match quarantine_malformed_sites(path) {
                Ok(quarantine) => eprintln!(
                    "[Hosting] Malformed hosted-site metadata {} quarantined at {}: {}",
                    path.display(),
                    quarantine.display(),
                    e
                ),
                Err(quarantine_err) => eprintln!(
                    "[Hosting] Malformed hosted-site metadata {} could not be quarantined: {}; starting with no hosted sites",
                    path.display(),
                    quarantine_err
                ),
            }
            Vec::new()
        }
    }
}

fn quarantine_malformed_sites(path: &Path) -> Result<PathBuf, String> {
    let quarantine = malformed_sites_quarantine_path(path)?;
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

fn malformed_sites_quarantine_path(path: &Path) -> Result<PathBuf, String> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("clock before UNIX_EPOCH: {e}"))?
        .as_secs();
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("hosted_sites.json");
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

/// Save all hosted sites to disk.
pub fn save_sites(sites: &[HostedSite]) {
    let Some(path) = metadata_path() else { return };
    if let Err(e) = save_sites_to_path(sites, &path) {
        eprintln!(
            "[Hosting] Failed to save hosted-site metadata {}: {}",
            path.display(),
            e
        );
    }
}

fn save_sites_to_path(sites: &[HostedSite], path: &Path) -> Result<(), String> {
    match std::fs::read(path) {
        Ok(data) => {
            if serde_json::from_slice::<Vec<HostedSite>>(&data).is_err() {
                return Err(format!(
                    "refusing to overwrite malformed hosted-site metadata at {}; fix or remove it manually",
                    path.display()
                ));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) if path.is_dir() => {}
        Err(e) => {
            return Err(format!(
                "refusing to overwrite unreadable hosted-site metadata at {}: {}; fix or remove it manually",
                path.display(),
                e
            ));
        }
    }
    let json = serde_json::to_string_pretty(sites)
        .map_err(|e| format!("serialize hosted-site metadata: {}", e))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            format!(
                "create hosted-site metadata directory {}: {}",
                parent.display(),
                e
            )
        })?;
    }
    std::fs::write(path, json)
        .map_err(|e| format!("write hosted-site metadata {}: {}", path.display(), e))
}

// ---------------------------------------------------------------------------
// ID generation
// ---------------------------------------------------------------------------

/// Generate an 8-character alphanumeric site ID.
pub fn generate_site_id() -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..8)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

// ---------------------------------------------------------------------------
// MIME type detection
// ---------------------------------------------------------------------------

/// Return the MIME type for a file extension.
pub fn mime_from_extension(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "xml" => "application/xml; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        "md" => "text/markdown; charset=utf-8",
        // Images
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "webp" => "image/webp",
        "avif" => "image/avif",
        // Fonts
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "eot" => "application/vnd.ms-fontobject",
        // Media
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        // Other
        "wasm" => "application/wasm",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_metadata_path(root: &Path) -> PathBuf {
        root.join("hosted_sites.json")
    }

    fn hosted_site_fixture() -> HostedSite {
        HostedSite {
            id: "abc12345".into(),
            name: "Test Site".into(),
            directory: "/tmp/sites/abc12345".into(),
            created_at: 1_700_000_000,
            files: vec![
                SiteFile {
                    path: "index.html".into(),
                    size: 1024,
                },
                SiteFile {
                    path: "css/style.css".into(),
                    size: 512,
                },
            ],
            relay_url: Some("http://127.0.0.1:8080/sites/abc12345/".into()),
            cdn_url: None,
        }
    }

    fn quarantined_metadata_paths(root: &Path) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = fs::read_dir(root)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("hosted_sites.json.malformed-"))
            })
            .collect();
        paths.sort();
        paths
    }

    #[test]
    fn test_generate_site_id_length() {
        let id = generate_site_id();
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn test_generate_site_id_unique() {
        let a = generate_site_id();
        let b = generate_site_id();
        assert_ne!(a, b);
    }

    #[test]
    fn test_mime_html() {
        assert_eq!(mime_from_extension("html"), "text/html; charset=utf-8");
        assert_eq!(mime_from_extension("HTML"), "text/html; charset=utf-8");
        assert_eq!(mime_from_extension("htm"), "text/html; charset=utf-8");
    }

    #[test]
    fn test_mime_css_js() {
        assert_eq!(mime_from_extension("css"), "text/css; charset=utf-8");
        assert_eq!(
            mime_from_extension("js"),
            "application/javascript; charset=utf-8"
        );
    }

    #[test]
    fn test_mime_images() {
        assert_eq!(mime_from_extension("png"), "image/png");
        assert_eq!(mime_from_extension("jpg"), "image/jpeg");
        assert_eq!(mime_from_extension("svg"), "image/svg+xml");
        assert_eq!(mime_from_extension("webp"), "image/webp");
    }

    #[test]
    fn test_mime_fonts() {
        assert_eq!(mime_from_extension("woff2"), "font/woff2");
        assert_eq!(mime_from_extension("ttf"), "font/ttf");
    }

    #[test]
    fn test_mime_unknown() {
        assert_eq!(mime_from_extension("xyz"), "application/octet-stream");
    }

    #[test]
    fn load_sites_missing_file_starts_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = test_metadata_path(dir.path());

        assert!(load_sites_from_path(&path).is_empty());
        assert!(!path.exists());
        assert!(quarantined_metadata_paths(dir.path()).is_empty());
    }

    #[test]
    fn load_sites_reads_valid_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = test_metadata_path(dir.path());
        let site = hosted_site_fixture();
        fs::write(&path, serde_json::to_vec(&vec![site.clone()]).unwrap()).unwrap();

        let sites = load_sites_from_path(&path);

        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].id, site.id);
        assert_eq!(sites[0].name, site.name);
        assert_eq!(sites[0].files.len(), site.files.len());
        assert!(path.exists());
        assert!(quarantined_metadata_paths(dir.path()).is_empty());
    }

    #[test]
    fn load_sites_quarantines_malformed_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = test_metadata_path(dir.path());
        let original = b"{not valid json";
        fs::write(&path, original).unwrap();

        assert!(load_sites_from_path(&path).is_empty());

        let quarantines = quarantined_metadata_paths(dir.path());
        assert_eq!(quarantines.len(), 1);
        assert!(!path.exists());
        assert_eq!(fs::read(&quarantines[0]).unwrap(), &original[..]);
    }

    #[test]
    fn load_sites_quarantines_invalid_utf8_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = test_metadata_path(dir.path());
        let original = vec![0xff, 0xfe, b'[', b']'];
        fs::write(&path, &original).unwrap();

        assert!(load_sites_from_path(&path).is_empty());

        let quarantines = quarantined_metadata_paths(dir.path());
        assert_eq!(quarantines.len(), 1);
        assert!(!path.exists());
        assert_eq!(fs::read(&quarantines[0]).unwrap(), original);
    }

    #[test]
    fn save_sites_refuses_to_overwrite_malformed_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = test_metadata_path(dir.path());
        let original = b"{not valid json";
        fs::write(&path, original).unwrap();

        let err = save_sites_to_path(&[hosted_site_fixture()], &path)
            .expect_err("malformed metadata should not be overwritten");

        assert!(err.contains("refusing to overwrite malformed hosted-site metadata"));
        assert_eq!(fs::read(&path).unwrap(), &original[..]);
        assert!(quarantined_metadata_paths(dir.path()).is_empty());
    }

    #[test]
    fn save_sites_refuses_to_overwrite_invalid_utf8_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = test_metadata_path(dir.path());
        let original = vec![0xff, 0xfe, b'[', b']'];
        fs::write(&path, &original).unwrap();

        let err = save_sites_to_path(&[hosted_site_fixture()], &path)
            .expect_err("invalid UTF-8 metadata should not be overwritten");

        assert!(err.contains("refusing to overwrite malformed hosted-site metadata"));
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(quarantined_metadata_paths(dir.path()).is_empty());
    }

    #[test]
    fn save_sites_to_path_persists_valid_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("hosted_sites.json");
        let site = hosted_site_fixture();

        save_sites_to_path(&[site.clone()], &path).expect("valid metadata should save");

        let loaded = load_sites_from_path(&path);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, site.id);
    }

    #[test]
    fn save_sites_to_path_surfaces_directory_creation_failure() {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("not-a-directory");
        fs::write(&parent, "blocking file").unwrap();
        let path = parent.join("hosted_sites.json");

        let err = save_sites_to_path(&[hosted_site_fixture()], &path)
            .expect_err("directory creation failure should surface");

        assert!(err.contains("create hosted-site metadata directory"));
    }

    #[test]
    fn save_sites_to_path_surfaces_write_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = test_metadata_path(dir.path());
        fs::create_dir(&path).unwrap();

        let err = save_sites_to_path(&[hosted_site_fixture()], &path)
            .expect_err("write failure should surface");

        assert!(err.contains("write hosted-site metadata"));
    }

    #[test]
    fn test_site_serialization() {
        let site = hosted_site_fixture();
        let json = serde_json::to_string(&site).unwrap();
        let back: HostedSite = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "abc12345");
        assert_eq!(back.files.len(), 2);
    }
}
