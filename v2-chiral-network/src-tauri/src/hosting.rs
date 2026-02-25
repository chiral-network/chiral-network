use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

/// Directory where all hosted sites are stored.
pub fn sites_base_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("chiral-network").join("sites"))
}

/// Path to the metadata JSON file.
fn metadata_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("chiral-network").join("hosted_sites.json"))
}

/// Load all hosted sites from disk.
pub fn load_sites() -> Vec<HostedSite> {
    let Some(path) = metadata_path() else {
        return Vec::new();
    };
    let Ok(data) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str(&data).unwrap_or_default()
}

/// Save all hosted sites to disk.
pub fn save_sites(sites: &[HostedSite]) {
    let Some(path) = metadata_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(sites) {
        let _ = std::fs::write(&path, json);
    }
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
        assert_eq!(mime_from_extension("js"), "application/javascript; charset=utf-8");
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
    fn test_site_serialization() {
        let site = HostedSite {
            id: "abc12345".into(),
            name: "Test Site".into(),
            directory: "/tmp/sites/abc12345".into(),
            created_at: 1700000000,
            files: vec![
                SiteFile { path: "index.html".into(), size: 1024 },
                SiteFile { path: "css/style.css".into(), size: 512 },
            ],
        };
        let json = serde_json::to_string(&site).unwrap();
        let back: HostedSite = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "abc12345");
        assert_eq!(back.files.len(), 2);
    }
}
